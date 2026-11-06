# Contributing to open-rally

Thanks for taking the time. This is a small project with a clear scope, so most of this document is about keeping it that way.

## Scope

open-rally is a shared board for one person's private network. Things that fit:

- Making posting, copying and downloading faster or more reliable
- Better behaviour on Linux and Windows (both are expected to work, only macOS is tested regularly)
- Hardening the existing security model without changing its defaults
- Fixing anything in the [deferred list](#known-gaps)

Things that do not fit, and will be declined politely:

- Accounts, permissions, multiple rooms
- Exposing the server to the public internet, tunnels, public URLs
- Message encryption or HTTPS termination inside the app (put a reverse proxy in front if you need it)
- Frontend frameworks or a build step for `assets/`

If you are unsure, open an issue first and describe the situation you are in rather than the feature you want. That makes it much easier to find the right change.

## Setting up

```bash
git clone https://github.com/GenesisSam/open-rally.git
cd open-rally
cargo test                          # everything, including real-socket tests
cargo run -- --serve                # dev server on http://127.0.0.1:7777
```

The dev server prints where its `config.toml` is. Use `OPEN_RALLY_DATA_DIR=/tmp/rally-dev` to keep test data out of your real board.

On Linux the default build needs GTK 3 and libayatana-appindicator development packages for the tray. If you only touch the server, build with `--no-default-features` and skip them.

Docker: `docker build -t open-rally .` and `docker run --rm -e OPEN_RALLY_PASSWORD=x -p 127.0.0.1:7777:7777 open-rally`.

## Making a change

1. **Write the test first.** Every behaviour in `src/` has a test that fails without it. Unit tests live next to the code; HTTP behaviour goes in `tests/api.rs` (router + `tower::oneshot`), anything that needs a real socket goes in `tests/server.rs`. Frontend changes are verified by hand in a browser; say what you checked in the PR.
2. **Keep the diff focused.** One behaviour per PR. Refactors go in their own PR.
3. **Run the whole suite both ways** before pushing:
   ```bash
   cargo test && cargo test --no-default-features
   ```
4. **No new runtime dependencies without a sentence on why.** The binary is meant to stay small and the dependency list auditable.
5. **Security defaults stay conservative.** A PR may add an option; it may not loosen what happens when no option is set. Anything touching `src/net.rs`, `src/auth.rs`, `src/server/middleware.rs` or `src/files.rs` needs a test for the input it changes.
6. **Commit messages** follow `type: short description` (`feat:`, `fix:`, `docs:`, `test:`, `chore:`). The body explains why when it is not obvious.

## Pull request checklist

- [ ] Tests added or updated, and they fail without the change
- [ ] `cargo test` and `cargo test --no-default-features` pass
- [ ] `cargo build` produces no warnings
- [ ] README updated if behaviour, configuration or defaults changed
- [ ] Frontend changes checked in at least one browser (say which)
- [ ] No real IP addresses, hostnames or secrets in code, docs or examples (use `100.100.100.100`)

## Reporting bugs

Include the platform, how you ran it (menubar app, `--serve`, Docker), what you did, what you expected, and what happened. Server output from `--serve` is the most useful thing you can attach. Please do not include your `config.toml`.

## Security issues

If you find something that lets a client on the network read or change data without the password, or lets a web page on a client machine do so, please report it privately through GitHub's security advisory form for this repository rather than a public issue.

## Known gaps

Small things that are known and welcome as first contributions:

- `uploads/<id>/` directories can be orphaned if the server is stopped mid-upload; a sweep on start would fix it
- Filenames longer than 255 bytes fail with a 500 instead of being truncated
- The first page loads only on scroll; if 100 posts fit on screen nothing more loads
- Reconnecting replaces the whole list instead of fetching only what was missed (`after=` already exists on the API)
- `lan_ip()` picks the first private IPv4 and may choose a VM bridge before the real interface
- `config.toml` is written with default permissions; `0600` would be better
- IPv4 only; `[::1]` is in the Host allowlist but never bound
