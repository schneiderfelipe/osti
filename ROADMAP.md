# Ostinato roadmap

This roadmap sketches where Ostinato is headed, starting from the pitch in [README.md](README.md) and the design goals in [DESIGN.md](DESIGN.md).

## Vision

Ostinato is a terminal, keyboard-driven, modal tool for composing music: a sequencer with looper features, good at writing note patterns, looping them, and playing them back on simple synthesized instruments.

Its editing philosophy follows Helix's lineage (selection-first, noun before verb) and, through Helix, Kakoune's selection model and Neovim's modal-editing and command-line heritage (see [DESIGN.md § Helix, Kakoune, and Neovim lineage](DESIGN.md#helix-kakoune-and-neovim-lineage)).

This document describes what "done" looks like for a first, minimal version of Ostinato (the MVP) and what comes after it.

## MVP

### Composing

Modal, keyboard-driven editing of note patterns on a grid that shows notes and time, selection-first and noun-before-verb wherever that fits notes and time instead of characters and lines.

### Tracks

A handful of tracks, each holding its own pattern, can be opened, switched between, and played together as a small arrangement.

### Playback and looping

Every track loops and plays back in real time, audibly and visibly in sync with the grid; transport (play, pause, seek) is always a few keystrokes away.

### Instruments

A small set of simple, synthesized instruments covers common sounds without tying Ostinato to one genre or one synthesis approach.

### Command line

A single command line handles non-editing operations (opening a track, loading, and saving a composition) with completion that doubles as live documentation of what's available.

### Undo

Every edit, typed or generated, feeds one action stream, so undo is a replay of that stream rather than a separate mechanism.

## Beyond MVP

### Macros

Recording and replaying stretches of the same action stream that backs undo.

### Which-key guidance

A popup that shows the keys available for a partially-typed sequence, so an unfamiliar or half-remembered binding is discoverable in the moment.

### Non-interactive rendering

Rendering a composition to an audio file from the command line, without opening the interactive session.

### Deeper sound and mixing

Expanded sound-shaping and mixing controls, kept secondary to the core work of writing and looping patterns.

### Handing off to specialized tools

Making it easy to send work to an existing, specialized tool instead of reimplementing that tool's job inside Ostinato.

## Non-goals

- A binary plugin system.
- An embedded scripting language.
- Matching dedicated DAWs at deep sound design or professional mixing.
