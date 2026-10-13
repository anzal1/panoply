# Security

Panoply opens files from anywhere, so parser bugs are security bugs.

## Reporting

Please report vulnerabilities privately through GitHub's
"Report a vulnerability" button on this repository (Security tab). Do not open
a public issue. Include a sample file or steps to reproduce where possible.

We aim to acknowledge reports within three days and to ship fixes for
confirmed issues as fast as their severity requires.

## Scope

- Crashes, memory corruption or code execution from a crafted file.
- A module reading files it was not given, or reaching the network without
  permission.
- Tampered model or engine downloads.
- Anything that sends user data off the device without consent.
