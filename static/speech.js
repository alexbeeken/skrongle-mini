// Shared speech-synthesis helper for everything that talks: Numbers by ear,
// Date Dash, Listening, Sprint mode and the tap-a-word-to-hear-it layer (say.js).
// The browser's own voices do the speaking, so there is no audio to host and
// any installed voice for the course's language can be picked. Exposes
// window.LearnSpeech; base.html loads it on every page, before the page's
// own script.
window.LearnSpeech = (function () {
  "use strict";
  var synth = window.speechSynthesis;
  var supported = !!(synth && window.SpeechSynthesisUtterance);

  function voiceLang(v) { return (v.lang || "").toLowerCase().replace("_", "-"); }

  // Voices whose language matches `lang` ("es" matches es-ES, es-MX, es_419…;
  // "pt-BR" prefers pt-BR but accepts any pt), best first: exact dialect,
  // the system default, then plain names ("Mónica") ahead of novelty voices
  // ("Grandpa (Spanish (Spain))"), then by name.
  function voicesFor(lang) {
    lang = (lang || "").trim();
    var primary = lang.toLowerCase().split(/[-_]/)[0];
    if (!supported || !primary) return [];
    var all = synth.getVoices().filter(function (v) { return voiceLang(v).split("-")[0] === primary; });
    var exact = lang.toLowerCase().replace("_", "-");
    function rank(v) { return (voiceLang(v) === exact ? 0 : 4) + (v.default ? 0 : 2) + (/\(/.test(v.name) ? 1 : 0); }
    all.sort(function (a, b) { var d = rank(a) - rank(b); return d !== 0 ? d : (a.lang + a.name).localeCompare(b.lang + b.name); });
    return all;
  }

  // Has the browser handed out any voices at all yet? (Chrome starts with an
  // empty list; a missing language is only certain once the list is in.)
  function anyVoices() { return supported && synth.getVoices().length > 0; }

  // Calls fn now and whenever the voice list changes. Some browsers hand out
  // an empty list first and never fire the event, hence the retries.
  function onVoices(fn) {
    fn();
    if (!supported) return;
    if (typeof synth.addEventListener === "function") synth.addEventListener("voiceschanged", fn);
    else if (typeof synth.onvoiceschanged !== "undefined") synth.onvoiceschanged = fn;
    setTimeout(fn, 600);
    setTimeout(fn, 2500);
  }

  // The voice with this URI, looked up afresh in the live list: Chrome
  // replaces its voice objects when the list changes, and a stale one is
  // silently ignored (the text then comes out in the system default voice).
  function voiceByURI(uri) {
    if (!supported || !uri) return null;
    var live = synth.getVoices();
    for (var i = 0; i < live.length; i++) if (live[i].voiceURI === uri) return live[i];
    return null;
  }

  // Remembered voice per language, shared by all the games.
  function savedVoice(lang) { try { return localStorage.getItem("speech:voice:" + lang); } catch (e) { return null; } }
  function saveVoice(lang, uri) { try { localStorage.setItem("speech:voice:" + lang, uri); } catch (e) { /* private mode */ } }

  // Fill a <select> with the voices for `lang`, restoring the saved choice
  // and saving changes. Returns a function that gives the voice to use now.
  function bindSelect(select, lang, label) {
    var key = lang.toLowerCase().split(/[-_]/)[0];
    function fill() {
      var voices = voicesFor(lang);
      var want = select.value || savedVoice(key);
      select.innerHTML = "";
      if (!voices.length) {
        var o = document.createElement("option");
        o.value = ""; o.textContent = t('common.js.no_voice', { language: label || lang });
        select.appendChild(o);
        return;
      }
      voices.forEach(function (v) {
        var o = document.createElement("option");
        o.value = v.voiceURI; o.textContent = v.name + " (" + v.lang + ")";
        if (v.voiceURI === want) o.selected = true;
        select.appendChild(o);
      });
    }
    select.addEventListener("change", function () { saveVoice(key, select.value); });
    onVoices(fill);
    return function () { return voiceByURI(select.value) || voicesFor(lang)[0] || null; };
  }

  var utter = null; // keep a reference: Chrome drops callbacks of collected utterances
  var speakTimer = null, speakSeq = 0;

  // Emoji, arrows and other pictographs are never read aloud ("🔊" would
  // come out as "speaker with three sound waves"). Unicode property escapes
  // where the browser has them, else the usual symbol blocks.
  var PICTO;
  try { PICTO = new RegExp("\\p{Extended_Pictographic}|[\\u{1F000}-\\u{1FAFF}\\u{2190}-\\u{21FF}\\u{FE0F}\\u{200D}\\u{20E3}]", "gu"); }
  catch (e) { PICTO = /[\u2190-\u21FF\u2300-\u23FF\u2460-\u24FF\u25A0-\u27BF\u2900-\u29FF\u2B00-\u2BFF\u3030\u303D\u3297\u3299\uFE0F\u200D\u20E3]|[\uD83C-\uD83E][\uDC00-\uDFFF]/g; }
  function stripEmoji(text) { return String(text == null ? "" : text).replace(PICTO, " ").replace(/\s+/g, " ").trim(); }

  // Split text into pieces of at most `max` words, breaking at punctuation
  // first ("Mi abuela está enferma hoy, | y no come nada." -> phrases, then
  // phrases longer than `max` words into runs of `max`).
  function chunks(text, max) {
    var out = [], phrase = [];
    var flush = function () {
      for (var i = 0; i < phrase.length; i += max) out.push(phrase.slice(i, i + max).join(" "));
      phrase = [];
    };
    String(text).split(/\s+/).forEach(function (word) {
      if (!word) return;
      phrase.push(word);
      if (/[,;:.!?…]$/.test(word)) flush();
    });
    flush();
    return out;
  }

  // Say `text` with opts {voice, lang, rate, chunk, gap, onstart, onend,
  // onerror(code)}. With `chunk` set, the text is spoken in pieces of at most
  // that many words with a pause of `gap` ms (default 350) between them:
  // the only way to get genuinely slow dictation, since Chrome squeezes the
  // rate range of system voices (on macOS, rate 0.25 is barely slower than 1).
  // Chrome (notably on macOS) also tends to speak the utterance that follows
  // a cancel() in the system default voice, i.e. in the wrong language. So:
  // only cancel when something is actually playing, and give the engine a
  // moment to settle before the next text.
  function speak(text, opts) {
    opts = opts || {};
    if (!supported) return;
    var seq = ++speakSeq;
    if (speakTimer) { clearTimeout(speakTimer); speakTimer = null; }
    var delay = 0;
    if (synth.speaking || synth.pending) { synth.cancel(); delay = 250; }
    text = stripEmoji(text);
    if (!text) { if (opts.onend) opts.onend(); return; }
    var pieces = opts.chunk > 0 ? chunks(text, opts.chunk) : [text];
    if (!pieces.length) return;
    var started = false;
    function finish() { if (opts.onend) opts.onend(); }
    function sayPiece(i) {
      if (seq !== speakSeq) return; // a newer request superseded this one
      var v = opts.voice || null;
      utter = new SpeechSynthesisUtterance(pieces[i]);
      if (v) utter.voice = v;
      utter.lang = v ? v.lang : (opts.lang || "");
      utter.rate = opts.rate || 1;
      utter.onstart = function () { if (!started) { started = true; if (opts.onstart) opts.onstart(); } };
      utter.onend = function () {
        if (seq !== speakSeq) return;
        if (i + 1 < pieces.length) speakTimer = setTimeout(function () { speakTimer = null; sayPiece(i + 1); }, opts.gap || 350);
        else finish();
      };
      utter.onerror = function (ev) {
        finish();
        if (opts.onerror && ev.error !== "interrupted" && ev.error !== "canceled") opts.onerror(ev.error);
      };
      synth.speak(utter);
    }
    speakTimer = setTimeout(function () { speakTimer = null; sayPiece(0); }, delay);
  }

  // Stop whatever is playing or about to play.
  function hush() {
    speakSeq += 1;
    if (speakTimer) { clearTimeout(speakTimer); speakTimer = null; }
    if (supported && (synth.speaking || synth.pending)) synth.cancel();
  }

  window.addEventListener("pagehide", hush);

  // ---- the audio switch and plain "say this" ----------------------------
  // One preference for the whole site (Sprint mode's speaker button, the
  // "can't listen now" link): off by hand, or off for a while.
  function audioOn() {
    try {
      if (localStorage.getItem("speech:audio") === "0") return false;
      var until = parseInt(localStorage.getItem("speech:audio-off-until") || "0", 10);
      return !(until && Date.now() < until);
    } catch (e) { return true; }
  }
  function setAudio(on) {
    try { localStorage.setItem("speech:audio", on ? "1" : "0"); localStorage.removeItem("speech:audio-off-until"); } catch (e) { /* private mode */ }
    if (!on) hush();
  }
  function muteFor(ms) {
    try { localStorage.setItem("speech:audio-off-until", String(Date.now() + ms)); } catch (e) { /* private mode */ }
    hush();
  }

  // The voice to use for `lang`: the one remembered by the games' voice
  // pickers, else the best installed one.
  function bestVoice(lang) {
    var key = (lang || "").toLowerCase().split(/[-_]/)[0];
    return voiceByURI(savedVoice(key)) || voicesFor(lang)[0] || null;
  }
  function hasVoice(lang) { return supported && voicesFor(lang).length > 0; }

  // Say `text` in `lang` with the remembered voice, if audio is on. opts:
  // {slow, rate, force (ignore the audio switch), onstart, onend, onerror}.
  // Returns whether anything was said.
  function say(text, lang, opts) {
    opts = opts || {};
    // "cansado / cansada" (a term with alternatives) is said as both, with a pause.
    text = stripEmoji(text).replace(/\s\/\s/g, ", ").trim();
    if (!supported || !text || (!opts.force && !audioOn())) return false;
    var how = opts.slow ? { rate: 0.6, chunk: 2, gap: 350 } : { rate: opts.rate || 0.95 };
    speak(text, { voice: bestVoice(lang), lang: lang, rate: how.rate, chunk: how.chunk, gap: how.gap,
      onstart: opts.onstart, onend: opts.onend, onerror: opts.onerror });
    return true;
  }

  return { supported: supported, voicesFor: voicesFor, anyVoices: anyVoices, onVoices: onVoices, voiceByURI: voiceByURI,
    bindSelect: bindSelect, speak: speak, hush: hush, chunks: chunks, stripEmoji: stripEmoji,
    audioOn: audioOn, setAudio: setAudio, muteFor: muteFor, bestVoice: bestVoice, hasVoice: hasVoice, say: say };
})();
