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

- **Hosting is in.** The phase ends with the server reachable over the internet,
  not only on a LAN.
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
| #65 | Each other investigator at the performer's location may commit one card to their skill test, asked in player order before the performer; a committed card's result addition is its committer's, a modifier the performer's, PR #1321; slices #1316, #1317, #1318, #1319, #1320 |
| #146 | The turn's player window reopens after each action (Appendix II 2.2.1), answered by every investigator holding an eligible fast play; a fast play, a 0-action ability, the act advance and End Turn open none, PR #1332; slices #1330, #1331 |
| #153 | An act's group clue spend asks each contributor with a choice for their share, in player order with the investigator who started the advance last and covering the rest; both `AdvanceAct` and The Barrier 01109's round-end spend, PR #1327 |
| #151 | An engagement tie is the lead's to answer, wherever an enemy comes to stand ready beside several investigators its prey can't tell apart, PR #1310; slices #1306, #1307, #1308, #1309 |
| #359 | Medical Texts 01035 chooses an investigator at your location before its test, and the result heals or damages that investigator; the pick crosses the skill test beside the ability source, PR #1326; slice #1325 |
| #371 | Location-attachment ownership, PR #984 |
| #381 | Controller-scoped reaction windows; closed in #1145's 2026-10-20 triage |
| #409 | A failed Fight deals the attack's damage to the investigator engaged with the attacked enemy, PR #1301 |
| #458 | Deterministic prompt ids: a prompt is named by its log position, with #1230, PR #1297 |
| #570 | Replay integrity; not planned, superseded by #1224's label-matched replay |
| #573 | Act clue thresholds scale per investigator; an act with no clue objective carries none, PR #1298 |
| #580 | Room registry: each game loads once, off the global lock and the async workers, and is evicted after 10 idle minutes, PR #1285; slices #1282, #1283, #1284 |
| #581 | Seat identity on the wire: every answer names its investigator, each account gets its own messages, with #1230, PR #1297 |
| #583 | Wire versioning: `Hello` carries the build, with #1230, PR #1297 |
| #642 | When two or more ST.7 results are pending, the performer picks which applies next; modifiers and the follow-up stay first and Retaliate last, PR #1315; slices #1312, #1313, #1314 |
| #662 | A barricade-stranded enemy engages the investigators left at the origin, PR #1299 |
| #795 | A hunter's prey tie offers only the directions it can move, per FFG's ruling, PR #1300 |
| #974 | Cover Up triggers for a co-located investigator, PR #984 |
| #975 | Co-location reach into another investigator's threat area, PR #984 |
| #1061 | Scrying 01061 reorders any investigator's deck, chosen once and kept for its placements; an eliminated investigator is never offered by a choice of investigator, PR #1326; slice #1324 |
| #1086 | A committed skill's ST.7 ability says whether it modifies the test's consequence or adds one; modifiers apply before the follow-up, both gated on the outcome, PR #1311 |
| #1132 | An eliminated investigator's card that later leaves play is removed from the game into their own pile, PR #1302 |
| #1139 | The Barrier advances on a two-seat combat tie, PR #1195 |
| #1140 | WebSocket liveness: heartbeat, explicit close codes, graceful shutdown and input lockout, PR #1290; slices #1286, #1287, #1288, #1289 |
| #1179 | `mount_board` for the web tests, PR #1260 |
| #1185 | The lead chooses who takes each Investigation turn while two or more investigators are waiting, PR #1305; slices #1303, #1304 |
| #1194 | Each answerer passes separately in a reaction window, with #1230, PR #1297 |
| #1224 | A stored game is its creation inputs plus a checked action log, PR #1264; slices #1262, #1263 |
| #1225 | Configurable bind, dist path and build id, PR #1261 |
| #1226 | Release artifact and deploy script, PR #1267 |
| #1227 | Nightly backups to R2, PR #1268 |
| #1229 | The live game, one pure core behind the server and the web harnesses, PR #1280 |
| #1230 | The wire break: prompt id, the answering investigator, the per-account message, PR #1297; slices #1291–#1296 |
| #1233 | One integration-test binary per crate, PR #1249; slices #1246, #1247, #1248 |
| #1234 | Board fixtures beside `Seat`, PR #1259; slices #1251–#1258 |
| #1236 | Accounts, sessions and the dev sign-in, PR #1271; slices #1269, #1270 |
| #1237 | GitHub and Google sign-in against the allowlist, PR #1277; slices #1275, #1276 |
| #1238 | Seating refuses a roster naming one investigator twice, PR #1281 |
| #1239 | The lobby: creating a game makes a lobby at its own link, any account opening it becomes a member, members take and give up picks live, and the creator orders the seating, moves the lead and starts; Start stores the investigator → account assignment beside the game, PR #1339; slices #1333–#1338 |
| #1240 | "Your games" lists the account's lobbies, the started games under "Needs you" and "In progress", the unresumable and the finished, each finished one opening on its final board, PR #1344; slices #1340–#1343 |
| #1241 | A member leaves a lobby or the creator removes one; the creator deletes a game, and reassigns an investigator mid-game; the board knows the creator and the members, PR #1349; slices #1345–#1348 |
| #1242 | One pure `layout` decides where the live prompt goes for each account: a strip headed with the name of each investigator of yours it asks, each with its own options and Pass in a window several answer; a spinner on the panel of one another account answers for; the lead's strip, the lead-waiting line, or the waiting strip above the board; the status bar's "Waiting on" is gone, PR #1353; slices #1351, #1352 |
| #1245 | `wasm-test` runs locally by default, engine diffs included, PR #1250 |
| #1272 | Phase-doc sync with the closed milestone issues, PR #1273 |
| #1274 | A sign-in returns to the page it started from, through any provider, the dev picker and "Try again"; a `return_to` that isn't a path on this site lands on `/`, PR #1350 |
| #1278 | Phase-doc sync with #1237, PR #1279 |
| #1323 | Choose an investigator and keep the choice: one card-effect primitive whose pick later steps read, across a skill test and a native's suspends, PR #1326; slices #1324, #1325 |

## Ordering

Lanes run in parallel. Within a lane, an arrow is a blocking edge on the
tracker, and the tracker's dependency view is authoritative.

**1. Start now.**

- **Glossary:** Account, Lobby, Creator, Member and viewer, landed with this
  plan.
- **Tests:** #1233 (✅ PR #1249) → #1234 (✅ PR #1259). #1179 (✅ PR #1260) follows
  #1233. The two-investigator engine tests of lane 3 copy #1234's fixtures.
- **Hosting:** #1225 (✅ PR #1261) and #1224 (✅ PR #1264) → #1226 (✅ PR #1267) →
  #1227 (✅ PR #1268).
- **Accounts:** #1236 (✅ PR #1271) → #1237 (✅ PR #1277).
- **Engine and server:** #1229 (✅ PR #1280), #1238 (✅ PR #1281), #580
  (✅ PR #1285).
- **Liveness:** #1140 (✅ PR #1290).

**2. The wire break: #1229 + #1234 → #1230 (✅ PR #1297).** One change to the
wire format. #458, #581, #583 and #1194 close with it.

**→ The next architecture review** (#1145 step 5) is due now that #1230 has
landed, before phase 9 is scoped.

**3. Rules fixes, after #1234.**

- Standalone: #573 (✅ PR #1298), #662 (✅ PR #1299), #795 (✅ PR #1300), #409
  (✅ PR #1301), #1132 (✅ PR #1302), #1185 (✅ PR #1305).
- Engagement: #151 (✅ PR #1310), card 07's tie frame.
- ST.7: #1086 (✅ PR #1311) → #642 (✅ PR #1315) → #65 (✅ PR #1321). #359
  (✅ PR #1326) and #1061 (✅ PR #1326) follow #1086 and pair on one
  choose-and-keep primitive, specced in #1323 (✅ PR #1326).
- After the wire break: #153 (✅ PR #1327, also after #573) and #146
  (✅ PR #1332).

**4. The lobby: #1236 (✅ PR #1271) + #1224 (✅ PR #1264) + #1230 (✅ PR #1297) →
#1239 (✅ PR #1339) → #1240 (✅ PR #1344) and #1241 (✅ PR #1349).** #1274
(✅ PR #1350), returning to the page a sign-in started from, follows #1237
(✅ PR #1277) and #1239 (✅ PR #1339).

**5. The table: #1230 (✅ PR #1297) → #1242 (✅ PR #1353) → #863 → #534 and
#1101. #1242 (✅ PR #1353) → #1243 and #1322.** #1322, the board surface for committed cards, also follows
#65 (✅ PR #1321), which records each card's committer.

**6. Reconnect and presence.** #587 and #582, after #1230 (✅ PR #1297), put the
full history in `Hello`. #1232 follows #1230 (✅ PR #1297), #1140 (✅ PR #1290)
and #1241 (✅ PR #1349).

**7. Hardening.** #584 after #1225 (✅ PR #1261), #585 after #1236 (✅ PR #1271),
#586 after #583 (✅ PR #1297), and #588 after #458 (✅ PR #1297).

**8. Exit:** a two-account Gathering on the deployed host, then #1244.

## Open questions

None. #1230's spec settles one question the rules don't answer: whether a pass
still stands after another investigator plays a Fast card.

## Dependencies

- Phase 5 (server + persistence): the WebSocket sync layer.
- Phase 6 (web client v0): the UI that hosts multiplayer.
- Phase 7 (The Gathering) and 7.5 (all five investigators): the scenario and the
  roster that are played over the wire.

## What "done" looks like

Two accounts on two machines sign in with OAuth at the hosted URL, gather in a
lobby, and play The Gathering through to a resolution, each seeing their own
investigators' prompts and "waiting for…" on the other's. Either one can drop
mid-game and reconnect to the same table, log included.
