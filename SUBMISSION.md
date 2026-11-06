# Submitting hermes-chan to the hackathon

Deadline: **5 PM EST, Nov 11 2026**. Three things must land: a catalog PR, a
demo video, and the entry form.

## 1. Publish the repo

```bash
cd /Users/gilang/ngoding/hermes-chan
git init
git add .
git commit -m "hermes-chan: living desktop companion for Hermes"
gh repo create AlleyBo55/hermes-chan --public --source=. --push
```

Do **not** commit build output (`overlay/src-tauri/target/`, `__pycache__/`) —
`.gitignore` already excludes them. The repo must be **reviewable source only**:
no compiled binary is shipped (the overlay builds on the user's machine), which
keeps us clear of the catalog's binary-attestation rule.

## 2. Pin the commit

```bash
git rev-parse HEAD        # full 40-char SHA
```

Put that SHA and the repo URL into `CATALOG_ENTRY.yaml` (`sha:` and `repo:`).
Branches, tags and short SHAs are rejected by the loader.

## 3. Open the catalog PR

```bash
# fork + clone NousResearch/hermes-agent, then:
cp CATALOG_ENTRY.yaml <fork>/plugin-catalog/hermes-chan.yaml
# commit on a branch and open the PR
```

In the PR description, restate the disclosure lines (local bridge server — an
owner-only AF_UNIX socket on macOS/Linux, a token-guarded 127.0.0.1 port on
Windows — spawns a built-from-source overlay, drives the screen through its Rust
native-input backend under Hermes approval, and moves the active window for
short pranks via System Events, SetWindowPos or EWMH). Reviewers turn these
into the entry's disclosure summary.

Before pushing, run the same check CI runs:

```bash
hermes plugins validate /Users/gilang/ngoding/hermes-chan --install-deps
```

Fix anything it flags. The `capabilities` block in both `plugin.yaml` and
`CATALOG_ENTRY.yaml` must match what `register(ctx)` registers (rule 5), or the
entry fails admission.

## 4. Record the demo (< 90 s)

Show the plugin working, not slides. Suggested beat sheet:

1. `/hermeschan wake` — she fades in and perches on the active window.
2. Ask the agent to do something real ("open that file", "switch the video") —
   she walks over and performs it via `computer_use`; the action fires for real.
3. `/hermeschan say` + type into her bubble — a round-trip chat.
4. `grab_window shake` — she grabs the real window and shakes it.
5. Leave it idle a beat — a heartbeat tick makes her peek/react on her own.

## 5. Submit

- Tweet the demo video, tag **@NousResearch**.
- Put the PR link + tweet link into the form: https://form.typeform.com/to/FwJLvOln
  (you can submit as soon as the PR is open).

## Admission checklist (copy into the PR)

- [ ] Source-only repo; no committed binary; `.gitignore` excludes build output
- [ ] `capabilities` matches `register(ctx)` (rule 5)
- [ ] `hermes plugins validate` clean, no `dangerous` scan findings (rule 6)
- [ ] No `.env`/credential reads; secrets only via Hermes (rule 12)
- [ ] Body never bypasses approvals; unattended runs don't spawn it (rule 11)
- [ ] Disclosure lines in README + PR (rule 13)
- [ ] `requires_hermes: ">=0.23.0"` truthful in plugin.yaml + entry (rule 14)
- [ ] Full 40-char SHA pin (rule 2)
