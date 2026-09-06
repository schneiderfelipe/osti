# `osti`

`osti`[^1] is a terminal, keyboard-driven, modal tool for composing music.

See [DESIGN.md](DESIGN.md) for the design goals and rationale, and [ROADMAP.md](ROADMAP.md) for where the project is headed.

## Developing

On Linux, cpal's ALSA backend needs ALSA's development headers to build (`libasound2-dev` on
Debian/Ubuntu, `alsa-lib-devel` on Fedora, `alsa-lib` on Arch).

```sh
cargo run
```

Before your first commit, point git at the repo's hooks so the checks CI runs also run locally:

```sh
git config core.hooksPath .githooks
```

[^1]: Short for **ostinato**, the musical term for a phrase repeated stubbornly, underneath everything else (which is exactly what a loop is).
      The full word was already taken on crates.io, so the command kept the nickname.
