<div align="center">
    <img src="resources/icons/hicolor/scalable/apps/icon.svg" width="250"/>
    <h1>Calendar</h1>
    <h3>Never miss a thing!</h3>
    <p>A calendar application for the COSMIC™ desktop</p>
</div>

![color-schemes-light.png](https://github.com/user-attachments/assets/df4f9b5f-e1d8-47b8-b2ae-82b1c0073068#gh-light-mode-only)
![color-schemes-dark.png](https://github.com/user-attachments/assets/9a7af840-2005-41b9-81d5-e18a31864663#gh-dark-mode-only)

## Installation

A [justfile](./justfile) is included by default for the [casey/just][just] command runner.

- `just` builds the application with the default `just build-release` recipe
- `just run` builds and runs the application
- `just install` installs the project into the system
- `just vendor` creates a vendored tarball
- `just build-vendored` compiles with vendored dependencies from that tarball
- `just check` runs clippy on the project to check for linter warnings
- `just check-json` can be used by IDEs that support LSP

## Online calendars

Calendar owns its account data and credentials through the embedded `accounts` library.
Open **Settings → Accounts** to sign in, reconnect, remove, or pause an account. No
separate Accounts installation or broker is required. Enabled accounts refresh
automatically; use the refresh button beside an account in the sidebar to retry
immediately.

Calendar ships with Google and Microsoft desktop OAuth registrations. To use a custom
registration, set `ACCOUNTS_GOOGLE_CALENDAR_CLIENT_ID` or
`ACCOUNTS_MICROSOFT_CALENDAR_CLIENT_ID` before launching Calendar. If the custom
registration requires a secret, also set the matching `_CLIENT_SECRET`. The default
loopback redirect is `http://127.0.0.1/callback`.

Microsoft accounts use Microsoft Graph and the delegated `Calendars.Read` permission.
If this permission was added after sign-in, use **Reconnect** in Calendar. Calendar
shows Microsoft event occurrences from roughly one year before today through one year
after today.

Sign-in fails entirely if secure storage is unavailable or locked; credentials are
never stored in plaintext or kept only in memory. Online calendars are read-only.
Calendar keeps the current sync results in memory. The separate notification worker
delivers alerts for local calendars, and the running Calendar app also checks alerts
for synced events.

The Flatpak uses the Secret portal for encrypted, app-private credential storage.

## Translators

[Fluent][fluent] is used for localization of the software. Fluent's translation files are found in the [i18n directory](./i18n). New translations may copy the [English (en) localization](./i18n/en) of the project, rename `en` to the desired [ISO 639-1 language code][iso-codes], and then translations can be provided for each [message identifier][fluent-guide]. If no translation is necessary, the message may be omitted.

## Packaging

If packaging for a Linux distribution, vendor dependencies locally with the `vendor` rule, and build with the vendored sources using the `build-vendored` rule. When installing files, use the `rootdir` and `prefix` variables to change installation paths.

```sh
just vendor
just build-vendored
just rootdir=debian/cosmic-ext-calendar prefix=/usr install
```

It is recommended to build a source tarball with the vendored dependencies, which can typically be done by running `just vendor` on the host system before it enters the build environment.

## Developers

Developers should install [rustup][rustup] and configure their editor to use [rust-analyzer][rust-analyzer]. To improve compilation times, disable LTO in the release profile, install the [mold][mold] linker, and configure [sccache][sccache] for use with Rust. The [mold][mold] linker will only improve link times if LTO is disabled.

The Linux CI build uses Rust 1.99.0 and Ubuntu 24.04. On Ubuntu, install
`pkg-config`, `libudev-dev`, `libwayland-dev`, and `libxkbcommon-dev`.
Use the committed lockfile for repeatable checks:

```sh
cargo test --locked
cargo clippy --locked --all-targets
cargo build --locked
```

[fluent]: https://projectfluent.org/
[fluent-guide]: https://projectfluent.org/fluent/guide/hello.html
[iso-codes]: https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes
[just]: https://github.com/casey/just
[rustup]: https://rustup.rs/
[rust-analyzer]: https://rust-analyzer.github.io/
[mold]: https://github.com/rui314/mold
[sccache]: https://github.com/mozilla/sccache
