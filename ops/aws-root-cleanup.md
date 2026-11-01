# AWS root cleanup

How to stop running the hosted AWS account as root (#973, audit finding
B2). At the end, nobody uses root for daily work, the old access keys are
gone, every person signs in with MFA, the deploy runs as an identity that
can touch only the Skrongle stack and its settings, and CloudTrail records
every change.

This file sits in `ops/`, not `docs/`, on purpose: everything in `docs/`
is built into the server and readable by anyone at `/docs`. Keep it here.

Skrongle mini has no AWS account. None of this applies to it.

## Before you start

It takes about an hour, mostly clicking in the console. Do it in one
sitting if you can. Each step leaves the account working, so you can also
stop after any step and carry on later.

What it costs, in total: nothing for IAM, Identity Center, Organizations,
the password policy and the account settings. CloudTrail's first trail of
management events is free; its log files in S3 cost a few cents a month.
GuardDuty (step 8, optional) is free for 30 days, then usually a dollar or
two a month for an account this small.

You need: the root sign-in and its MFA device, a terminal in the repo's top
folder (every command below runs from there), and the AWS CLI 2.32 or
newer (`aws --version`; `brew upgrade awscli`).

The names used below. Change them everywhere if you pick others.

| Name | What it is |
|---|---|
| `skrongle` | today's CLI profile: an `aws login` session of the root user |
| `skrongle-admin` | the new CLI profile for admin work (step 3) |
| `skrongle-deploy` | the new CLI profile the deploy uses (step 5) |
| `alex` | your new Identity Center user |
| `SkrongleDeploy` | the permission set the deploy runs as |
| `skrongle-audit` | the CloudTrail trail, and the stack that holds it |
| `learnmap` | the app's stack (`AWS_STACK_NAME`), in `us-west-2` |

Why Identity Center and not an IAM user with a key: Identity Center gives
short sessions that expire on their own and sign in through the browser
with MFA. There is no long-lived key on the laptop to leak. The IAM user
`skrongle-deploy` that exists today is deleted in step 7.

The order puts CloudTrail (step 4) before the switch and the deletes, so
the cleanup itself is in the log.

In every terminal you use for this, first run:

```sh
export AWS_REGION=us-west-2
```

## 1. Take stock

Read only. Nothing here changes the account.

- [ ] Check today's session works. If it says the session expired, run
  `aws login --profile skrongle` and try again.

  ```sh
  aws sts get-caller-identity --profile skrongle
  ```

  The `Arn` ends in `:root`. That is the problem this runbook fixes.

- [ ] See every user and key, with MFA and last use, in one table:

  ```sh
  aws iam generate-credential-report --profile skrongle
  aws iam get-credential-report --profile skrongle --query Content --output text | base64 -d | cut -d, -f1,4,5,8,9,10,11,14,15,16 | column -s, -t
  ```

  If the second command says the report is not ready, wait ten seconds and
  run it again. The columns are: user, password on, password last used,
  MFA, key 1 active, key 1 made, key 1 last used, key 2 active, key 2 made,
  key 2 last used. The line `<root_account>` is root. The audit
  (2026-10-25) saw: root with an active key from 2022-02-19 and MFA on;
  `admin` with an active key last used in 2016 and no MFA;
  `skrongle-deploy` with no key.

- [ ] Write down the last-used date of the root key and of the `admin`
  key. The settings in Parameter Store were first written on 2026-10-24.
  If neither key was used on or after that day, those keys could not have
  read the settings, and you do not need to rotate them for this reason
  (see "After this runbook").

- [ ] See what each user may do:

  ```sh
  for u in $(aws iam list-users --profile skrongle --query 'Users[].UserName' --output text); do echo "== $u"; aws iam list-attached-user-policies --profile skrongle --user-name "$u" --query 'AttachedPolicies[].PolicyName' --output text; aws iam list-user-policies --profile skrongle --user-name "$u" --query 'PolicyNames' --output text; done
  ```

- [ ] See where the deploy's credentials come from. The deploy scripts
  load `deploy/deploy.env`, but a variable exported in the shell wins over
  the file.

  ```sh
  env | grep '^AWS_' | sed 's/=.*/=(set)/'
  grep -n '^AWS_PROFILE=' deploy/deploy.env
  grep -c '^AWS_ACCESS_KEY_ID=.' deploy/deploy.env
  grep -n '^\[\|login_session\|sso_' ~/.aws/config
  ls ~/.aws/credentials
  ```

  Expected today: `AWS_PROFILE=skrongle` in `deploy/deploy.env`, `0` keys
  in it, `login_session = arn:aws:iam::<account>:root` under
  `[profile skrongle]`, and no `~/.aws/credentials` file. If the
  credentials file exists, look at which profiles it names; a key there is
  one more thing to delete in step 7.

- [ ] See whether the app's stack exists yet:

  ```sh
  aws cloudformation describe-stacks --profile skrongle --stack-name learnmap --query 'Stacks[0].StackStatus' --output text
  ```

  "does not exist" means the first deploy is still to come. Step 6 then
  does the first deploy instead of an update.

- [ ] See whether a trail exists already (the audit saw none):

  ```sh
  aws cloudtrail describe-trails --profile skrongle --query 'trailList[].[Name,HomeRegion,IsMultiRegionTrail]' --output text
  ```

## 2. Switch off the stale keys

Deactivating is reversible. Nothing local uses these keys (step 1 showed
the CLI uses an `aws login` session, not a key), so nothing should break.
If something does, switch the key back on and find out what used it. The
keys are deleted for good in step 7. Free.

- [ ] Root key. Sign in at <https://console.aws.amazon.com/> as the root
  user. Account menu (top right) > Security credentials > Access keys >
  select the key > Actions > Deactivate.

- [ ] The `admin` user's key. In the same console: IAM > Users > `admin` >
  Security credentials > Access keys > Actions > Deactivate. Or from the
  terminal:

  ```sh
  for k in $(aws iam list-access-keys --profile skrongle --user-name admin --query 'AccessKeyMetadata[].AccessKeyId' --output text); do aws iam update-access-key --profile skrongle --user-name admin --access-key-id "$k" --status Inactive; done
  ```

## 3. An admin who is not root, with MFA

IAM Identity Center with one user. Enabling it creates an AWS
Organization with this account as its management account; that is what
lets Identity Center give access to the account. Free.

- [ ] Still signed in as root, set the console's region (top right) to US
  West (Oregon) us-west-2. Identity Center lives in the region it is
  enabled in.

- [ ] IAM Identity Center > Enable > "Enable IAM Identity Center with AWS
  Organizations" > Enable. Wait until the dashboard shows.

- [ ] Settings > Authentication > Multi-factor authentication >
  Configure. Choose "Every time they sign in (always-on)", tick both
  "Security keys and built-in authenticators" and "Authenticator apps",
  and choose "Require them to register an MFA device at sign in". Save.

- [ ] Users > Add user. Username `alex`, your e-mail, first and last
  name, "Send an email to this user with password setup instructions".
  Add user. Open the e-mail, accept, set a password and register an MFA
  device.

- [ ] Permission sets > Create permission set > Predefined permission set
  > AdministratorAccess > Next. Session duration 1 hour. Next > Create.

- [ ] AWS accounts > tick this account > Assign users or groups > Users >
  `alex` > Next > `AdministratorAccess` > Next > Submit.

- [ ] Settings > note the "AWS access portal URL"
  (`https://d-xxxxxxxxxx.awsapps.com/start`). This is where you sign in
  from now on, instead of the root sign-in.

- [ ] Make the admin CLI profile:

  ```sh
  aws configure sso
  ```

  Answer: SSO session name `skrongle`; SSO start URL the access portal
  URL; SSO region `us-west-2`; registration scopes: press Enter. Approve
  in the browser. Pick this account and `AdministratorAccess`; default
  client region `us-west-2`; output format `json`; profile name
  `skrongle-admin`.

- [ ] Check it:

  ```sh
  aws sts get-caller-identity --profile skrongle-admin --query Arn --output text
  ```

  You should see `arn:aws:sts::<account>:assumed-role/AWSReservedSSO_AdministratorAccess_<hex>/alex`.
  From here on, sign out of root. Use the access portal and
  `--profile skrongle-admin`.

## 4. Turn on CloudTrail

One multi-region trail of management events (every create, change and
delete, in every region, plus sign-ins), with log file validation, into a
private bucket that deletes logs after 365 days.

It is a stack of its own (`deploy/aws/audit-trail.yaml`), not part of the
app's `deploy/aws/cloudformation.yaml`. The deploy can update and delete
the app's stack, so anything in it could be switched off by the deploy
identity, and the log is meant to record what that identity does. The
deploy policy (step 5) gives no CloudTrail rights at all.

Cost: the first copy of management events is free. You pay S3 for the
log files: a few cents a month at this account's size. No data events, no
Insights, no CloudWatch Logs and no KMS key, since each of those costs
extra. Log files hold the account's API calls and the IP addresses they
came from, no learner data, so a year is fine under the privacy notice.

- [ ] Create the trail:

  ```sh
  aws cloudformation deploy --profile skrongle-admin --region us-west-2 --stack-name skrongle-audit --template-file deploy/aws/audit-trail.yaml
  ```

- [ ] Check it is logging. Within about 15 minutes `LatestDeliveryTime`
  has a time and `LatestDeliveryError` is empty:

  ```sh
  aws cloudtrail get-trail-status --profile skrongle-admin --name skrongle-audit --query '{Logging:IsLogging,LastDelivery:LatestDeliveryTime,Error:LatestDeliveryError}'
  ```

If you would rather click: CloudTrail > Trails > Create trail. Name
`skrongle-audit`; create a new S3 bucket; untick "Log file SSE-KMS
encryption" (a KMS key costs $1 a month); keep "Log file validation"
ticked; leave CloudWatch Logs off. Next. Management events, Read and
Write; no data events, no Insights. Create trail. Console trails are
multi-region. Then open the bucket > Management > Create lifecycle rule,
apply to all objects, "Expire current versions of objects" after 365
days, "Permanently delete noncurrent versions" after 30 days. The
template does the same in one command, so prefer it.

## 5. A deploy identity that can do only the deploy

A second permission set, `SkrongleDeploy`, with the policy in
`deploy/aws/deploy-policy.json`. It allows what `scripts/deploy-aws.sh`
and `scripts/aws-env.sh` do, and nothing else:

- CloudFormation: change sets, describe and delete, on the `learnmap`
  stack only.
- EC2 in us-west-2: the security group, the instance, its IAM role link,
  the static IP, tags, describe calls, and the boot console the deploy
  pins the ssh host key from.
- IAM: create and change roles and instance profiles named
  `learnmap-*` (the stack's instance role and snapshot role), pass them
  to EC2 and Data Lifecycle Manager only, and attach only the AWS
  snapshot policy.
- Data Lifecycle Manager: the nightly snapshot policy.
- S3: create and configure buckets named `learnmap-*` (backups and
  exports), list them, and read backups under `daily/` (`--status`,
  `--backup` and `--restore` do). It may never write or delete an object
  in any bucket, and never read learners' exports.
- Parameter Store: read, write and delete under `/skrongle/` (the app's
  settings), and read the public Amazon Linux image id the stack looks up.
- KMS: only through Parameter Store and EC2, which is how the AWS managed
  keys for SecureString settings and the encrypted disk are used.

The backup service on the instance needs nothing from this identity. It
runs as the instance role in `deploy/aws/cloudformation.yaml`, which may
only put objects under `daily/` in the backup bucket.

What this policy cannot stop. It can create a role named `learnmap-*`
with any inline policy and start an instance with it, so someone holding
a deploy session could give themselves more rights. A permissions
boundary on the stack's roles would close that; it needs a template
change and is left for later. The session lasts at most 4 hours and needs
your MFA, and CloudTrail records it. EC2 rights are not limited to the
stack's resources, so a deploy session could also touch the old VPCs and
security groups in this account (see "After this runbook").

The file assumes the stack `learnmap`, the region `us-west-2` and the
settings under `/skrongle/`. If you change any of them, edit the file
first.

- [ ] Fill in the account id and copy the policy to the clipboard:

  ```sh
  ACCOUNT_ID=$(aws sts get-caller-identity --profile skrongle-admin --query Account --output text)
  sed "s/ACCOUNT_ID/$ACCOUNT_ID/g" deploy/aws/deploy-policy.json > "$TMPDIR/skrongle-deploy-policy.json"
  grep -c "$ACCOUNT_ID" "$TMPDIR/skrongle-deploy-policy.json"
  pbcopy < "$TMPDIR/skrongle-deploy-policy.json"
  ```

  The `grep -c` prints 8.

- [ ] Signed in through the access portal as `alex` (AdministratorAccess):
  IAM Identity Center > Permission sets > Create permission set > Custom
  permission set > Next. Open "Inline policy", replace the text with the
  clipboard. Next. Name `SkrongleDeploy`, session duration 4 hours (a
  build plus deploy can take a while). Next > Create.

- [ ] AWS accounts > tick this account > Assign users or groups > Users >
  `alex` > Next > `SkrongleDeploy` > Next > Submit.

- [ ] Make the deploy CLI profile. Run `aws configure sso` again. It
  offers the `skrongle` session; take it. Pick this account and
  `SkrongleDeploy`; region `us-west-2`; `json`; profile name
  `skrongle-deploy`.

## 6. Switch the deploy and prove it works

- [ ] In `deploy/deploy.env` set these three lines, and leave
  `AWS_SECRET_ACCESS_KEY=` empty too:

  ```sh
  AWS_PROFILE=skrongle-deploy
  AWS_ACCESS_KEY_ID=
  AWS_REGION=us-west-2
  ```

- [ ] Make sure the shell does not override it. This must print nothing
  but `AWS_REGION=us-west-2`:

  ```sh
  env | grep '^AWS_'
  ```

  If it shows `AWS_PROFILE=skrongle` or a key, remove that export from
  `~/.zshrc` and open a new terminal.

- [ ] Sign in for the day (it opens the browser; MFA every time):

  ```sh
  aws sso login --profile skrongle-deploy
  aws sts get-caller-identity --profile skrongle-deploy --query Arn --output text
  ```

  You should see `.../AWSReservedSSO_SkrongleDeploy_<hex>/alex`.

- [ ] The settings still read and the check still passes:

  ```sh
  scripts/aws-env.sh list
  scripts/aws-env.sh check
  ```

- [ ] Deploy. If the stack does not exist yet, this is the first deploy;
  otherwise it updates in place.

  ```sh
  scripts/deploy-aws.sh
  ```

  It ends with `==> released: https://...` and both health checks at
  HTTP 200.

- [ ] Take a backup now. This also proves the deploy may read backups:

  ```sh
  scripts/deploy-aws.sh --backup
  scripts/deploy-aws.sh --status
  ```

- [ ] Prove the identity is narrow. Both must fail with `AccessDenied`:

  ```sh
  aws iam list-users --profile skrongle-deploy
  aws s3 ls --profile skrongle-deploy
  ```

If the deploy stops with an access error, the message names the action.
For a stack failure, this shows the reason:

```sh
aws cloudformation describe-stack-events --profile skrongle-deploy --stack-name learnmap --query 'StackEvents[?contains(ResourceStatus, `FAILED`)].[LogicalResourceId,ResourceStatusReason]' --output text
```

Add the missing action to `deploy/aws/deploy-policy.json`, commit it, and
paste the new policy into the permission set: Permission sets >
`SkrongleDeploy` > Inline policy > Edit > Save. The console then asks to
update the account it is assigned to; say yes. Run the deploy again.

## 7. Delete the old ways in

Do this only after step 6 passed.

- [ ] Root access key. Sign in as root. Account menu > Security
  credentials > Access keys > the inactive key > Actions > Delete >
  type the key id > Delete.

- [ ] Root MFA. On the same page, "Multi-factor authentication (MFA)"
  lists at least one device. Add a second one (a security key or a
  second authenticator app) so losing one device does not lock you out.
  Free. Then sign out of root. From now on root is only for the few
  tasks only root can do (closing the account, changing the root e-mail
  or the support plan, restoring a lost admin).

- [ ] The `admin` user. Through the access portal as `alex`: IAM > Users >
  tick `admin` > Delete > type the name > Delete. The console removes its
  key, policies and password with it.

- [ ] The `skrongle-deploy` IAM user. Same page: tick
  `skrongle-deploy` > Delete. The `SkrongleDeploy` permission set
  replaces it, and the user has never had a key.

- [ ] If step 1 found a `~/.aws/credentials` file, delete each key in it
  that belonged to root or `admin` (they no longer work), or the whole
  file if nothing else is in it.

- [ ] Forget the local root session, then delete the `[profile
  skrongle]` block (two lines) from `~/.aws/config` with an editor:

  ```sh
  aws logout --profile skrongle
  ```

## 8. Account settings (cheap, worth it)

- [ ] Password policy. Identity Center has its own; this one covers any
  IAM user who ever gets a password. Free.

  ```sh
  aws iam update-account-password-policy --profile skrongle-admin --minimum-password-length 14 --require-symbols --require-numbers --require-uppercase-characters --require-lowercase-characters --allow-users-to-change-password --password-reuse-prevention 24
  ```

- [ ] Security contact, so AWS can reach you about abuse or a leaked key.
  Console: account menu > Account > Alternate contacts > Edit > Security
  contact: your name, e-mail and phone. Free.

- [ ] Block public S3 access for the whole account. Nothing Skrongle
  uses is public. If an old project serves files from a public bucket,
  this stops that. Free.

  ```sh
  aws s3control put-public-access-block --profile skrongle-admin --account-id "$(aws sts get-caller-identity --profile skrongle-admin --query Account --output text)" --public-access-block-configuration BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true
  ```

- [ ] Encrypt every new disk in us-west-2 by default. The stack's disk
  already is. Free.

  ```sh
  aws ec2 enable-ebs-encryption-by-default --profile skrongle-admin --region us-west-2
  ```

- [ ] GuardDuty in us-west-2 (optional). It reads CloudTrail, network
  and DNS logs and flags things like the instance talking to a known bad
  host, or calls from an odd place. Free for 30 days. Before the trial
  ends, GuardDuty > Usage shows what it would cost a month; for one small
  instance expect about a dollar or two. If that is too much, Settings >
  Disable. It only helps if you look: open GuardDuty > Findings once a
  week.

  ```sh
  aws guardduty create-detector --profile skrongle-admin --region us-west-2 --enable
  ```

- [ ] Budget. The $10 monthly budget from `docs/aws-setup.md` step 2 may
  go off once GuardDuty bills. Billing > Budgets > the budget > Edit >
  amount 15. A budget that only sends alerts is free.

## 9. Check that it is all done

Each command, as admin, and what it must show.

- [ ] Root has no key and has MFA. Prints `0` and `1`:

  ```sh
  aws iam get-account-summary --profile skrongle-admin --query 'SummaryMap.[AccountAccessKeysPresent,AccountMFAEnabled]' --output text
  ```

- [ ] No IAM users are left (prints nothing), so there are no long-lived
  keys at all:

  ```sh
  aws iam list-users --profile skrongle-admin --query 'Users[].UserName' --output text
  ```

- [ ] The deploy runs as `SkrongleDeploy`. Prints
  `.../AWSReservedSSO_SkrongleDeploy_<hex>/alex`:

  ```sh
  aws sts get-caller-identity --profile skrongle-deploy --query Arn --output text
  ```

- [ ] The trail is multi-region, validated and logging. Prints
  `skrongle-audit True True` and then `True` with no error:

  ```sh
  aws cloudtrail describe-trails --profile skrongle-admin --trail-name-list skrongle-audit --query 'trailList[].[Name,IsMultiRegionTrail,LogFileValidationEnabled]' --output text
  aws cloudtrail get-trail-status --profile skrongle-admin --name skrongle-audit --query '[IsLogging,LatestDeliveryError]' --output text
  ```

- [ ] The trail's bucket is private and expires logs. Prints four
  `True` and the rule `expire-logs` with 365 days:

  ```sh
  TRAIL_BUCKET=$(aws cloudformation describe-stacks --profile skrongle-admin --stack-name skrongle-audit --query "Stacks[0].Outputs[?OutputKey=='TrailBucket'].OutputValue" --output text)
  aws s3api get-public-access-block --profile skrongle-admin --bucket "$TRAIL_BUCKET" --output text
  aws s3api get-bucket-lifecycle-configuration --profile skrongle-admin --bucket "$TRAIL_BUCKET" --query 'Rules[].[ID,Expiration.Days]' --output text
  ```

- [ ] CloudTrail saw the deploy. Shows recent `PutParameter` or
  `GetParametersByPath` calls with the user `alex` (the deploy session),
  not root. Events appear about 5 to 15 minutes after the call:

  ```sh
  aws cloudtrail lookup-events --profile skrongle-admin --region us-west-2 --lookup-attributes AttributeKey=EventSource,AttributeValue=ssm.amazonaws.com --max-results 5 --query 'Events[].[EventTime,EventName,Username]' --output text
  ```

- [ ] A day later, the log files are intact. Prints that every digest
  and log file is valid:

  ```sh
  aws cloudtrail validate-logs --profile skrongle-admin --trail-arn "$(aws cloudtrail describe-trails --profile skrongle-admin --trail-name-list skrongle-audit --query 'trailList[0].TrailARN' --output text)" --start-time "$(date -u -v-1d +%Y-%m-%dT%H:%M:%SZ)"
  ```

- [ ] The account settings stuck. Prints the policy with `14`, four
  `True`, `True`, and your security contact:

  ```sh
  aws iam get-account-password-policy --profile skrongle-admin --query 'PasswordPolicy.MinimumPasswordLength'
  aws s3control get-public-access-block --profile skrongle-admin --account-id "$(aws sts get-caller-identity --profile skrongle-admin --query Account --output text)" --output text
  aws ec2 get-ebs-encryption-by-default --profile skrongle-admin --region us-west-2 --output text
  aws account get-alternate-contact --profile skrongle-admin --alternate-contact-type SECURITY
  ```

- [ ] GuardDuty, if you turned it on. Prints one detector id:

  ```sh
  aws guardduty list-detectors --profile skrongle-admin --region us-west-2 --output text
  ```

## After this runbook

- Tick the boxes on #973 and close it. Strike B2 from the audit's list
  of launch blockers.
- `docs/aws-setup.md` step 3 still tells you to make an IAM user
  `learnmap-deploy` with broad policies and an access key. Change it to
  point at the `SkrongleDeploy` permission set and
  `deploy/aws/deploy-policy.json`.
- Rotate the provider secrets only if step 1 showed the root or `admin`
  key used on or after 2026-10-24. Then follow step 3 of
  `ops/incident-response.md` for every provider. `SKRONGLE_SECRET_KEY` is
  the exception: it may only change while no learner has saved an AI key
  (before the first deploy, or on an empty database).
- Still open on #973: move Skrongle to its own member account, or delete
  the old projects' VPCs, security groups and roles here. Until then the
  deploy session can reach them. This lists the world-open security
  groups in every region (read only):

  ```sh
  for r in $(aws ec2 describe-regions --profile skrongle-admin --query 'Regions[].RegionName' --output text); do echo "== $r"; aws ec2 describe-security-groups --profile skrongle-admin --region "$r" --filters Name=ip-permission.cidr,Values=0.0.0.0/0 --query 'SecurityGroups[].[GroupId,GroupName,VpcId]' --output text; done
  ```

  The app's own group (`learnmap-SecurityGroup-...`) is open on 80 and
  443 by design.

## Sources

- Root access keys: <https://docs.aws.amazon.com/IAM/latest/UserGuide/id_root-user_manage_delete-key.html>
- Credential report: <https://docs.aws.amazon.com/IAM/latest/UserGuide/id_credentials_getting-report.html>
- Enable Identity Center: <https://docs.aws.amazon.com/singlesignon/latest/userguide/enable-identity-center.html>
- Organization and account instances (why it creates an organization): <https://docs.aws.amazon.com/singlesignon/latest/userguide/identity-center-instances.html>
- Identity Center MFA: <https://docs.aws.amazon.com/singlesignon/latest/userguide/mfa-configure.html>
- `aws configure sso`: <https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-sso.html>
- `aws login` and `aws logout`: <https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-sign-in.html>
- CloudTrail bucket policy: <https://docs.aws.amazon.com/awscloudtrail/latest/userguide/create-s3-bucket-policy-for-cloudtrail.html>
- CloudTrail trail in CloudFormation: <https://docs.aws.amazon.com/AWSCloudFormation/latest/TemplateReference/aws-resource-cloudtrail-trail.html>
- Log file validation: <https://docs.aws.amazon.com/awscloudtrail/latest/userguide/cloudtrail-log-file-validation-cli.html>
- CloudTrail pricing: <https://aws.amazon.com/cloudtrail/pricing/>
- GuardDuty pricing: <https://aws.amazon.com/guardduty/pricing/>
- IAM actions, resources and condition keys per service: <https://docs.aws.amazon.com/service-authorization/latest/reference/reference_policies_actions-resources-contextkeys.html>
- S3 account public access block: <https://docs.aws.amazon.com/AmazonS3/latest/userguide/configuring-block-public-access-account.html>
