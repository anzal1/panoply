# 0004. OS engines first, own small models second

- Status: accepted
- Date: 2026-10-13

## Context

Modern operating systems ship strong on-device engines for OCR, segmentation, faces, speech, translation and media encoding. Using them costs us no download and no resident memory and runs on the device's AI hardware.

## Decision

For every ML or acceleration job, use the OS engine when present and when it passes the module's quality bar. Otherwise use our smallest clean-licensed model, downloaded on first use and unloaded after 60 s idle. CI decides per platform by running the quality bar test.

## Consequences

A typical Mac user downloads about 170 MB of models in total even using every module; a Linux user using everything in every language about 1 GB. Behavior can differ slightly by platform, so quality bars are tested per platform.
