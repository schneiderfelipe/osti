# `osti` roadmap

This roadmap sketches where `osti` is headed, starting from the pitch in [README.md](README.md) and the design goals in [DESIGN.md](DESIGN.md).

Each checklist item below is scoped to a single pull request: concrete and demoable, but silent on how it's implemented.
PRs are expected to check off, split, reorder, or rewrite items here as work proceeds (this file tracks progress, it isn't a frozen spec).

## Vision

`osti` is a terminal, keyboard-driven, modal tool for composing music: a sequencer with looper features, good at writing note patterns, looping them, and playing them back on simple synthesized instruments.

Its editing philosophy follows Helix's lineage (selection-first, noun before verb) and, through Helix, Kakoune's selection model and Neovim's modal-editing and command-line heritage (see [DESIGN.md § Helix, Kakoune, and Neovim lineage](DESIGN.md#helix-kakoune-and-neovim-lineage)).

This document describes what "done" looks like for a first, minimal version of `osti` (the MVP) and what comes after it.

## MVP

### Foundation

- [x] Set up the project skeleton: an empty terminal UI and an audio loop, wired together and running
- [x] Play a single note back in a loop, audibly and visibly, confirming UI and audio stay in sync

### Composing

- [x] Show a grid of notes and time for one pattern
- [x] Move a selection around the grid
- [x] Add and remove notes at the current selection
- [ ] Change a selected note's pitch and duration
- [x] Select and edit more than one note at once

### Tracks

- [ ] Hold more than one pattern at a time, each on its own track
- [ ] Switch which track is being edited
- [ ] Play all tracks together, in sync

### Playback and looping

- [x] Play, pause, and seek without leaving the keyboard
- [ ] Loop a pattern continuously
- [ ] Change where a loop starts and ends

### Instruments

- [x] Play notes through one simple synthesized instrument
- [ ] Choose which instrument a track uses
- [ ] Add a second, differently-voiced instrument

### Command line

- [x] Open a command line for non-editing operations
- [ ] Save a composition to a file
- [ ] Load a composition from a file
- [ ] Open a specific track by name
- [ ] Show completions for available commands

### Undo

- [x] Undo the most recent edit
- [x] Redo an undone edit
- [x] Undo and redo across a run of edits

## Beyond MVP

- [ ] Record and replay a stretch of edits as macros
- [ ] Show which-key guidance for a partially-typed key sequence
- [ ] Render a composition to an audio file from the command line (no interactive UI needed)
- [ ] Deepen sound design and mixing, kept secondary to writing and looping patterns
- [ ] Hand off to specialized tools instead of reimplementing their job

## Non-goals

- A binary plugin system.
- An embedded scripting language.
- Deep sound design or professional mixing on par with dedicated DAWs.
