# Chat block markdown Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render headings and nested ordered/unordered lists in chat messages.

**Architecture:** Extend the existing pure `Markdown.blocks` parser and `MarkdownText` renderer; keep inline parsing and fenced code rendering intact.

**Tech Stack:** Swift 6, SwiftUI, Swift Testing.

**Spec:** `docs/superpowers/specs/2026-10-22-chat-markdown-design.md`

## Global Constraints

- No external markdown dependency; preserve unsupported source text.
- Preserve selectable text, fenced code handling, and inline-only markdown semantics.
- Test with `cd ui/menubar && ./scripts/test.sh`.
- Implement in a dedicated worktree from `feature/roadmap`; do not commit, push, or merge. Report branch, diff, and checks.

## Review Focus

- Nested lists and blank lines must not merge unrelated blocks; pin with parser tests.
- Fenced code contents must not be interpreted as headings/lists; test mixed blocks.
- Malformed syntax must remain readable; test fallback behavior.

---

### Task 1: Parse block markdown

**Files:**
- Modify: `ui/menubar/Sources/HerdbotKit/Timeline.swift`
- Test: `ui/menubar/Tests/HerdbotKitTests/ChatTests.swift`

**Interfaces:** Extend existing `Markdown.blocks(_:)` block representation for heading and list blocks, retaining existing text/code behavior.

- [x] Add tests for heading levels 1–6, ordered/unordered lists, nested indentation, inline text, blank-line boundaries, fenced code, malformed markers, and unsupported-source retention.
- [x] Run `cd ui/menubar && ./scripts/test.sh`; confirm new parser cases fail.
- [x] Implement the smallest parser extension that returns typed heading/list blocks and leaves unrecognized lines visible.
- [x] Run `cd ui/menubar && ./scripts/test.sh`; expect all tests pass.

### Task 2: Render block markdown

**Files:**
- Modify: `ui/menubar/Sources/HerdbotMenu/MessageViews.swift`

**Interfaces:** `MarkdownText` renders the extended block cases; no new public API.

- [x] Render headings with semantic font sizes/weights and list items with stable indentation and ordered markers.
- [x] Keep inline markdown and fenced code rendering unchanged; preserve `.textSelection(.enabled)`.
- [x] Run `cd ui/menubar && ./scripts/test.sh`; expect all tests pass.
- [ ] Manually inspect headings, nested lists, code, and mixed blocks in a chat reply.
