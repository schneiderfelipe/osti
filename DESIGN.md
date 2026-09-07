# `osti` design

This document describes the design goals for `osti`, including rationale.
It's heavily inspired by [Kakoune's](https://github.com/mawww/kakoune/blob/master/doc/design.asciidoc).

## Interactivity

`osti` is built for interactive use first: the loop playing audibly and the grid reflecting every edit visibly, in real time, is the whole premise.
Non-interactive use, like rendering a composition to an audio file from the command line, can follow, but it's secondary.

## Limited scope

`osti` is a sequencer with looper features.
It should be very good at writing tracks, looping them, and playing simple synthesized instruments over them.
Being merely adequate at deep sound design or professional mixing is an acceptable trade for staying sharp at that core job.

## Composability

`osti` should not try to own everything a serious music setup needs.
Where an existing, specialized tool already does a job well, `osti` should make it easy to hand off to that tool instead of reimplementing it.

## Orthogonality

Orthogonality is an ideal, not an absolute.
Only a handful of modes modify; command mode is for non-editing operations (loading and saving, opening a track).
Transport (play, pause, seek) isn't selection manipulation either, but it stays bound in normal mode anyway: speed's few-keystrokes requirement wins here over the ideal.
Commands should not be redundant with each other.

## Speed

`osti` should be fast to use (a handful of keystrokes for common tasks like chord entry, transpose, or toggling the loop, not many) and fast to execute.
The one hard real-time constraint the whole design turns on: the audio thread must never miss a deadline, regardless of what the terminal UI is doing at that moment.
That asymmetry should decide any tradeoff between UI responsiveness and audio stability in the audio thread's favor, always.

## Simplicity

Simplicity correlates with orthogonality and speed.
It makes the system easier to reason about, bugs easier to find, and the codebase easier to change.

- **Minimal threading.**
  One thread pair (UI thread and audio thread) joined by a single lock-free channel.
- **No binary plugin system.**
- **No embedded scripting language.**
  The command line covers what a scripting language would otherwise be reached for.
- **Limited smartness.**
  Where `osti` tries to be smart, it should offer a plain, non-smart alternative.
  Smart behavior should never be the only path.

## Unified interactive use and scripting

This follows from orthogonality and simplicity: normal mode isn't a layer of keybindings on top of a separate editing language, it *is* the editing language.
A key, a typed command, and a recorded macro all dispatch the same action; no internal command exists that a key merely happens to be bound to.
Generated edits go through that same action stream, which is what makes undo and macros just a recording of ordinary use, not a second thing to design and maintain.

## Instrument-agnostic

`osti` should not be tailored to one genre, and separately, it should not be tailored to one synthesis approach.

## Self-documenting

An unfamiliar or half-remembered keybinding should be discoverable inside the session, not by leaving it to check a reference.
The command line's completion should double as live documentation of what's available, and a which-key-style popup for partially-typed key sequences should exist from early on.

## Helix, Kakoune, and Neovim lineage

`osti` borrows its editing philosophy from Helix (selection-first, noun before verb) wherever that philosophy applies cleanly to notes and time instead of characters and lines.
Helix is itself a synthesis: Kakoune's selection model filtered through Vim and Neovim's modal-editing and command-line heritage.
`osti` draws on Kakoune and Neovim the same way Helix does (through that synthesis, not as separate influences to reconcile on its own).
But self-consistency inside `osti`'s own domain wins whenever the analogy strains.
