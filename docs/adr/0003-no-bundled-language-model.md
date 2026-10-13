# 0003. No bundled language model

- Status: accepted
- Date: 2026-10-13

## Context

A general language model costs 1 to 4 GB of download and memory, against the lowest-memory rule. People already use AI assistants, and those assistants can call Panoply through MCP.

## Decision

Panoply ships no chatbot model. Text-smart features (summarize, ask your PDF, meeting notes, dictation cleanup) use the OS model (Apple Foundation Models, Gemini Nano, Phi Silica), else the user's own local model or key with an explicit network label, else they are hidden.

## Consequences

Zero model cost for most users. These features are unavailable on devices without an OS model unless the user brings one.
