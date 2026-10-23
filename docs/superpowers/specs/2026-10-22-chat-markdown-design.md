# Chat block markdown

## Intent

Render common block markdown in chat replies so headings and lists read as structure instead of plain text, while retaining current inline markdown and fenced code behavior.

## Goals

- Render ATX headings, unordered lists, ordered lists, and nested list indentation in message bodies.
- Preserve inline emphasis/links, selectable text, fenced code blocks, and horizontal scrolling for long code lines.
- Treat malformed or unsupported markdown as readable text; do not crash or hide content.

## Non-goals

- No external markdown dependency, full CommonMark implementation, tables, raw HTML, or editing support.
- No changes to history storage or message layout beyond block rendering.

## Approaches

- Replace the small block splitter with a full markdown package: broader compatibility, but adds dependency and rendering complexity.
- Extend the existing lightweight `Markdown.blocks` parser and SwiftUI renderer for the named constructs: chosen because it reuses current code and bounds behavior to the requested gap.
- Render all content as plain text: safe but does not solve the usability issue.

## Constraints and builder choices

- The current `MarkdownText` is in `ui/menubar/Sources/HerdbotMenu/MessageViews.swift`; parser/model code is in HerdbotKit `Timeline.swift`.
- Keep parsing pure and testable in HerdbotKit. Use semantic SwiftUI text styles and indentation; avoid custom colors.
- Preserve original textual content for unsupported constructs.

## Verification

Unit tests cover heading levels, ordered/unordered/nested lists, inline markdown within list items, blank-line boundaries, fenced code adjacency, malformed markers, and exact source text preservation for unsupported blocks. Manually inspect representative chat replies in the app.
