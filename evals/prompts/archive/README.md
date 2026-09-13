# Superseded instruction sets

Each trial records the prompt filename as `instruction_set_version`.
Keep archived prompts unchanged so recorded trials resolve to the text that produced them.

`just config` reads `prompts/<name>.md`; it does not select files from this directory.

| File                 | Change                                     | Reason archived                                        |
| -------------------- | ------------------------------------------ | ------------------------------------------------------ |
| `c10r-first.md`      | Required a c10r query before file search   | Agents continued to rely on grep                       |
| `c10r-substitute.md` | Replaced duplicate file searches with c10r | Replaced by output limits and paging                   |
| `c10r-bounded.md`    | Added `--limit` and cursor paging          | Wrong trace syntax caused 35 of 45 trace calls to fail |

All three use the invalid form `c10r trace <relation> <name>`.
The valid form is `c10r trace <name> --relation <relation>`.

Renaming a prompt after a trial uses it requires a store migration.
Update `instruction_set_version`, recompute `trial_key`, and update `sweep-meta.json`; otherwise, a later import can duplicate the trial.

`c10r-dev-refined` was named `c10r-recover` during the dev replication.
The migration retagged its 80 recorded trials.
