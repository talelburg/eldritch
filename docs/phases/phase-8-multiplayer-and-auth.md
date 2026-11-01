# Phase 8 — Multiplayer + auth

## Status

🟡 **In progress** (planned 2026-10-30). Every row below is a filed issue on the
[milestone](https://github.com/talelburg/eldritch/milestone/9), and its blocking
edges are wired on the tracker. The plan came out of the wayfinder map
[#1210](https://github.com/talelburg/eldritch/issues/1210). Each decision lives
in the resolution comment of the map's ticket that made it, and each slice's
body links the ticket it came from.

## Goal

Friends on their own machines sign in with OAuth at a hosted URL and play The
Gathering together, and any of them can drop and reconnect mid-game.

## Scope

- **Hosting is in.** The phase ends with the server reachable over the
  internet, not only on a LAN.
- **Any assignment of investigators to accounts, with 1–4 investigators.** The
  solo case, one account holding two investigators, is a case of the same model.
- **A rules deferral is in** if it is observable in a 2–4 investigator Gathering
  played with implemented cards.
- **A review refactor is in only when a phase-8 feature consumes it.** Review
  items 02, 03, 04, 06, 27, 28, card 07's tie frame (#151) and card 08 (#1086)
  pass that test. The rest stay on #1145.
- **Hardening is in when it bites** once the server is on the internet or two
  clients share a game. CI and dependency hygiene (#589, #590) are out.

## Issues

**Open:** every issue named in the Ordering below that isn't marked ✅.

### Closed

| Issue | Work |
|---|---|
| #371 | Location-attachment ownership, PR #984 |
| #381 | Controller-scoped reaction windows; closed in #1145's 2026-10-20 triage |
| #570 | Replay integrity; not planned, superseded by #1224's label-matched replay |
| #580 | Room registry: each game loads once, off the global lock and the async workers, and is evicted after 10 idle minutes, PR #1285; slices #1282, #1283, #1284 |
| #974 | Cover Up triggers for a co-located investigator, PR #984 |
| #975 | Co-location reach into another investigator's threat area, PR #984 |
| #1139 | The Barrier advances on a two-seat combat tie, PR #1195 |
| #1179 | `mount_board` for the web tests, PR #1260 |
| #1224 | A stored game is its creation inputs plus a checked action log, PR #1264; slices #1262, #1263 |
| #1225 | Configurable bind, dist path and build id, PR #1261 |
| #1226 | Release artifact and deploy script, PR #1267 |
| #1227 | Nightly backups to R2, PR #1268 |
| #1229 | The live game, one pure core behind the server and the web harnesses, PR #1280 |
| #1233 | One integration-test binary per crate, PR #1249; slices #1246, #1247, #1248 |
| #1234 | Board fixtures beside `Seat`, PR #1259; slices #1251–#1258 |
| #1236 | Accounts, sessions and the dev sign-in, PR #1271; slices #1269, #1270 |
| #1237 | GitHub and Google sign-in against the allowlist, PR #1277; slices #1275, #1276 |
| #1238 | Seating refuses a roster naming one investigator twice, PR #1281 |
| #1245 | `wasm-test` runs locally by default, engine diffs included, PR #1250 |

## Ordering

Lanes run in parallel. Within a lane, an arrow is a blocking edge on the
tracker, and the tracker's dependency view is authoritative.

**1. Start now.**

- **Glossary:** Account, Lobby, Creator, Member and viewer, landed with this plan.
- **Tests:** #1233 (✅ PR #1249) → #1234 (✅ PR #1259). #1179 (✅ PR #1260) follows #1233. The two-investigator engine tests
  of lane 3 copy #1234's fixtures.
- **Hosting:** #1225 (✅ PR #1261) and #1224 (✅ PR #1264) → #1226 (✅ PR #1267) → #1227 (✅ PR #1268).
- **Accounts:** #1236 (✅ PR #1271) → #1237 (✅ PR #1277).
- **Engine and server:** #1229 (✅ PR #1280), #1238 (✅ PR #1281), #580 (✅ PR #1285).
- **Liveness:** #1140.

**2. The wire break: #1229 + #1234 → #1230.** One change to the wire format.
#458, #581, #583 and #1194 close with it. Until #1239 lands, a dev account
holds every investigator.

**→ The next architecture review** (#1145 step 5) runs once #1230 lands, before
phase 9 is scoped.

**3. Rules fixes, after #1234.**

- Standalone: #573, #662, #795, #409, #1132, #1185.
- Engagement: #151, card 07's tie frame.
- ST.7: #1086 → #642 → #65. #359 and #1061 follow #1086 and pair on one
  choose-and-keep primitive.
- After the wire break: #153 (also after #573) and #146 (after #1194).

**4. The lobby: #1236 + #1224 + #1230 → #1239 → #1240 and #1241.** #1241 is
rarely used, so it comes late and stays small. #1274, returning to the page a
sign-in started from, follows #1237 and #1239.

**5. The table: #1230 → #1242 → #863 → #534 and #1101. #1242 → #1243.**

**6. Reconnect and presence.** #587 and #582, after #1230, put the full history
in `Hello`. #1232 follows #1230, #1140 and #1241.

**7. Hardening.** #584 after #1225, #585 after #1236, #586 after #583, and #588
after #458.

**8. Exit:** a two-account Gathering on the deployed host, then #1244.

## Open questions

None. #1230's spec settles one question the rules don't answer: whether a pass
still stands after another investigator plays a Fast card.

## Dependencies

- Phase 5 (server + persistence): the WebSocket sync layer.
- Phase 6 (web client v0): the UI that hosts multiplayer.
- Phase 7 (The Gathering) and 7.5 (all five investigators): the scenario and
  the roster that are played over the wire.

## What "done" looks like

Two accounts on two machines sign in with OAuth at the hosted URL, gather in a
lobby, and play The Gathering through to a resolution, each seeing their own
investigators' prompts and "waiting for…" on the other's. Either one can drop
mid-game and reconnect to the same table, log included.
