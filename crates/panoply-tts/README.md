# panoply-tts

Text to speech for the `text.to_speech` module.

Engine ladder (docs/DECISIONS.md): the operating system first.

- macOS: `AVSpeechSynthesizer.write(_:toBufferCallback:)`, through `panoply-platform-macos`. The voices are the ones installed in System Settings. Nothing is ever played: the buffers go to a file.
- Windows: `Windows.Media.SpeechSynthesis.SpeechSynthesizer` to a stream, written to a file.
- Linux: no engine. Piper and Kokoro both depend on espeak-ng (GPL-3.0) for phonemes, so neither can ship. The tool is hidden on Linux. See docs/KNOWN-LIMITATIONS.md.

Long text is split into chunks of whole sentences, rendered one after another and appended to the file as they arrive, with progress after each chunk and a cancel check between chunks and between audio buffers. The output is 16 bit PCM WAV. M4A and MP3 are made from that WAV by the shared media engine.
