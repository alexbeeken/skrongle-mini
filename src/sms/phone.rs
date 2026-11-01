//! The dev phone: in dev mode without `SKRONGLE_SMS_PROVIDERS` the text
//! message chain is one `dev-phone` provider that keeps what it is sent in
//! memory, so a flow that texts a code can be clicked through and read back
//! at `/dev/phone` (routes/dev.rs), as e-mail is at `/dev/inbox`.
//!
//! The outbox, the job runner that sends it and the chain run as in production; only
//! the provider at the end is the app's. It also logs, as `log` does. Only
//! the newest [`KEEP`] texts are kept, and a restart empties it.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use super::{opted_out, Outgoing, Provider, SendError};

/// Texts kept; older ones are dropped.
pub const KEEP: usize = 200;

/// One text in the phone's conversation with a number: received from the
/// app, or (`from_phone`) a reply typed on the phone's page.
pub struct Received {
    pub id: i64,
    pub received_at: i64,
    /// The outbox row it came from; 0 for a reply.
    pub outbox_id: i64,
    pub to: String,
    pub text: String,
    pub purpose: String,
    /// Typed on the phone's page (`/dev/phone/reply`): the number texting the
    /// app. The app reads none of them; STOP and START act as a carrier
    /// would ([`Phone::reply`]), the rest make it read like a conversation.
    pub from_phone: bool,
}

impl Received {
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id, "received_at": self.received_at, "outbox_id": self.outbox_id,
            "to": self.to, "text": self.text, "purpose": self.purpose, "from_phone": self.from_phone,
        })
    }
}

/// The texts received, newest last.
pub struct Phone {
    messages: Mutex<VecDeque<Arc<Received>>>,
    next_id: AtomicI64,
    /// Numbers that replied STOP: texts to them are refused as a carrier
    /// refuses them (Twilio's 21610), until they reply START (#969).
    stopped: Mutex<HashSet<String>>,
}

/// Replies that opt a number out of texts, and back in, as US carriers read them.
const STOP_WORDS: &[&str] = &["STOP", "STOPALL", "UNSUBSCRIBE", "CANCEL", "END", "QUIT"];
const START_WORDS: &[&str] = &["START", "YES", "UNSTOP"];

impl Phone {
    pub fn new() -> Arc<Phone> {
        Arc::new(Phone { messages: Mutex::new(VecDeque::new()), next_id: AtomicI64::new(1), stopped: Mutex::new(HashSet::new()) })
    }

    /// What the phone received (not its replies), newest first; only those
    /// to a number containing `to` (digits only compared, so `+44 7700`
    /// matches `+447700…`) when given.
    pub fn list(&self, to: Option<&str>) -> Vec<Arc<Received>> {
        let want = to.map(digits);
        let all = self.messages.lock().unwrap();
        all.iter().rev().filter(|m| !m.from_phone && want.as_deref().is_none_or(|w| digits(&m.to).contains(w))).cloned().collect()
    }

    pub fn get(&self, id: i64) -> Option<Arc<Received>> {
        self.messages.lock().unwrap().iter().find(|m| m.id == id).cloned()
    }

    /// The newest text that came from outbox row `outbox_id`.
    pub fn by_outbox(&self, outbox_id: i64) -> Option<i64> {
        self.messages.lock().unwrap().iter().rev().find(|m| m.outbox_id == outbox_id).map(|m| m.id)
    }

    /// The conversation with `to`, oldest first: the texts it received and
    /// its replies.
    pub fn thread(&self, to: &str) -> Vec<Arc<Received>> {
        self.messages.lock().unwrap().iter().filter(|m| m.to == to).cloned().collect()
    }

    /// A reply typed on the phone's page: `to` texts the app. Its id. STOP
    /// (and its synonyms) opts the number out of texts, START back in.
    pub fn reply(&self, to: &str, text: &str) -> i64 {
        let word = text.trim().to_ascii_uppercase();
        if STOP_WORDS.contains(&word.as_str()) {
            self.stopped.lock().unwrap_or_else(|p| p.into_inner()).insert(digits(to));
        } else if START_WORDS.contains(&word.as_str()) {
            self.stopped.lock().unwrap_or_else(|p| p.into_inner()).remove(&digits(to));
        }
        self.store(Received { id: 0, received_at: crate::db::now(), outbox_id: 0, to: to.into(), text: text.into(), purpose: "reply".into(), from_phone: true })
    }

    /// Delete one text; whether it was there.
    pub fn remove(&self, id: i64) -> bool {
        let mut all = self.messages.lock().unwrap();
        let before = all.len();
        all.retain(|m| m.id != id);
        all.len() != before
    }

    /// Forget the whole conversation with `to` (its texts and the replies);
    /// the ids that went.
    pub fn remove_thread(&self, to: &str) -> Vec<i64> {
        let mut all = self.messages.lock().unwrap();
        let gone = all.iter().filter(|m| m.to == to).map(|m| m.id).collect();
        all.retain(|m| m.to != to);
        gone
    }

    pub fn clear(&self) {
        self.messages.lock().unwrap().clear();
        self.stopped.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }

    /// `to` replied STOP and not START since.
    pub fn is_stopped(&self, to: &str) -> bool {
        self.stopped.lock().unwrap_or_else(|p| p.into_inner()).contains(&digits(to))
    }

    fn push(&self, sms: &Outgoing) -> i64 {
        self.store(Received { id: 0, received_at: crate::db::now(), outbox_id: sms.id, to: sms.to.clone(), text: sms.text.clone(), purpose: sms.purpose.clone(), from_phone: false })
    }

    fn store(&self, mut m: Received) -> i64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        m.id = id;
        let mut all = self.messages.lock().unwrap();
        all.push_back(Arc::new(m));
        while all.len() > KEEP {
            all.pop_front();
        }
        id
    }
}

/// The digits of a number, the + and spaces dropped.
fn digits(s: &str) -> String {
    s.chars().filter(char::is_ascii_digit).collect()
}

/// The `dev-phone` provider: hands each text to the phone.
pub struct PhoneProvider {
    phone: Arc<Phone>,
}

pub fn provider(phone: &Arc<Phone>) -> Box<dyn Provider> {
    Box::new(PhoneProvider { phone: phone.clone() })
}

impl Provider for PhoneProvider {
    fn name(&self) -> &str {
        "dev-phone"
    }

    fn send(&self, sms: &Outgoing) -> Result<(), SendError> {
        if self.phone.is_stopped(&sms.to) {
            tracing::info!("dev-phone: refused sms #{} ({}): the number replied STOP", sms.id, sms.purpose);
            return Err(opted_out("21610 attempt to send to unsubscribed recipient"));
        }
        let id = self.phone.push(sms);
        tracing::info!("dev-phone: received sms #{} ({}); read it at /dev/phone/{id}", sms.id, sms.purpose);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phone_keeps_what_it_is_sent() {
        let phone = Phone::new();
        let p = provider(&phone);
        let send = |id: i64, to: &str, text: &str| p.send(&Outgoing { id, to: to.into(), text: text.into(), purpose: "test".into() }).unwrap();
        send(4, "+447700900123", "Code 1");
        send(5, "+15550100", "Hi");
        send(6, "+447700900123", "Code 2");
        assert_eq!(p.name(), "dev-phone");
        assert_eq!(phone.list(None).iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["Code 2", "Hi", "Code 1"]);
        assert_eq!(phone.list(Some("+44 7700")).len(), 2);
        assert_eq!(phone.thread("+447700900123").iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["Code 1", "Code 2"]);
        // A reply is in the conversation, not in the inbox list.
        let reply = phone.reply("+447700900123", "Thanks");
        assert!(phone.get(reply).unwrap().from_phone);
        assert_eq!(phone.list(None).len(), 3);
        assert_eq!(phone.thread("+447700900123").iter().map(|m| (m.text.as_str(), m.from_phone)).collect::<Vec<_>>(), [("Code 1", false), ("Code 2", false), ("Thanks", true)]);
        let id = phone.by_outbox(5).unwrap();
        assert_eq!(phone.get(id).unwrap().to, "+15550100");
        assert!(phone.remove(id));
        assert!(!phone.remove(id));
        assert_eq!(phone.by_outbox(5), None);
        phone.clear();
        assert!(phone.list(None).is_empty());
    }

    #[test]
    fn stop_refuses_texts_as_opted_out_until_start() {
        let phone = Phone::new();
        let p = provider(&phone);
        let sms = Outgoing { id: 1, to: "+15555550123".into(), text: "Code".into(), purpose: "test".into() };
        phone.reply("+1 555 555 0123", " stop ");
        let err = p.send(&sms).unwrap_err();
        assert!(super::super::is_opted_out(&err), "{err}");
        assert!(phone.list(None).is_empty());
        phone.reply("+15555550123", "Start");
        p.send(&sms).unwrap();
        assert_eq!(phone.list(None).len(), 1);
    }
}
