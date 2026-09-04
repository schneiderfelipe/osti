# osti roadmap

This roadmap sketches where osti is headed, starting from the pitch in [README.md](README.md) and the design goals in [DESIGN.md](DESIGN.md).

Each checklist item below is scoped to a single pull request: concrete and demoable, but silent on how it's implemented.
PRs are expected to check off, split, reorder, or rewrite items here as work proceeds (this file tracks progress, it isn't a frozen spec).

## Vision

osti is a terminal, keyboard-driven, modal tool for composing music: a sequencer with looper features, good at writing note patterns, looping them, and playing them back on simple synthesized instruments.

Its editing philosophy follows Helix's lineage (selection-first, noun before verb) and, through Helix, Kakoune's selection model and Neovim's modal-editing and command-line heritage (see [DESIGN.md § Helix, Kakoune, and Neovim lineage](DESIGN.md#helix-kakoune-and-neovim-lineage)).

This document describes what "done" looks like for a first, minimal version of osti (the MVP) and what comes after it.

## MVP

### Foundation

- [ ] Set up the project skeleton: an empty terminal UI and an audio loop, wired together and running
- [ ] Play a single note back in a loop, audibly and visibly, confirming UI and audio stay in sync

### Composing

- [ ] Show a grid of notes and time for one pattern
- [ ] Move a selection around the grid
- [ ] Add and remove notes at the current selection
- [ ] Change a selected note's pitch and duration
- [ ] Select and edit more than one note at once

### Tracks

- [ ] Hold more than one pattern at a time, each on its own track
- [ ] Switch which track is being edited
- [ ] Play all tracks together, in sync

### Playback and looping

- [ ] Play, pause, and seek without leaving the keyboard
- [ ] Loop a pattern continuously
- [ ] Change where a loop starts and ends

### Instruments

- [ ] Play notes through one simple synthesized instrument
- [ ] Choose which instrument a track uses
- [ ] Add a second, differently-voiced instrument

### Command line

- [ ] Open a command line for non-editing operations
- [ ] Save a composition to a file
- [ ] Load a composition from a file
- [ ] Open a specific track by name
- [ ] Show completions for available commands

### Undo

- [ ] Undo the most recent edit
- [ ] Redo an undone edit
- [ ] Undo and redo across a run of edits

## Beyond MVP

- [ ] Macros: record and replay a stretch of edits
- [ ] Which-key guidance: show the keys available for a partially-typed sequence
- [ ] Non-interactive rendering: render a composition to an audio file from the command line
- [ ] Deeper sound and mixing, kept secondary to writing and looping patterns
- [ ] Handing off to specialized tools instead of reimplementing their job

## Non-goals

- A binary plugin system.
- An embedded scripting language.
- Matching dedicated DAWs at deep sound design or professional mixing.
