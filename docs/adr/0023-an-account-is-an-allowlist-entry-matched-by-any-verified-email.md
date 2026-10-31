# An Account is an allowlist entry, matched by any verified email

Sam is on the list as `sam@gmail.com`. On Tuesday Sam signs in with GitHub, whose verified emails are `sam@work.com` and `sam@gmail.com`. On Thursday Sam signs in with Google as `sam@gmail.com`. Sam expects to be the same person at the table both nights, holding the same investigators in the same games.

**An allowlist entry is an Account: a stable id, a display name the host sets, and one or more emails. Any sign-in whose provider-verified email matches one of them is that account, whichever provider it came through.** Decided in #1213.

- **Any verified email admits**, not only GitHub's primary one. That is what makes Sam one account on both nights, with no linking step.
- **No email is on two entries.** `server accounts add` refuses one another entry holds, naming that entry, so a sign-in never matches two accounts. Emails are stored lowercased, so the check and the lookup agree whatever case a provider reports.
- **The name at the table is the entry's display name.** The name a provider reports is ignored.
- **Removing an entry disables it** rather than deleting it, so stored games that refer to its id stay intact.

The allowlist is the `accounts` and `account_emails` tables in the server's SQLite, edited over ssh with `server accounts` (`docs/deploy.md`). There is no in-app admin role: the admin is whoever can ssh into the box.

## Considered options

**Keying an account on the provider's user id** (GitHub's numeric id, Google's `sub`), with the email used only to check the allowlist, was rejected. It is the conventional choice, and it makes Sam two accounts, one per provider, until a linking step joins them, which a friends-scale site would have to build and explain. Provider user ids are not stored at all. The cost is that whoever controls a verified address on the list is that account, so a friend who loses an address tells the host, who edits the entry.
