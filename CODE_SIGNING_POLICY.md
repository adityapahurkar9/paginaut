# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by
[SignPath Foundation](https://signpath.org).

## What is signed

The Windows installer (`Paginaut_<version>_x64-setup.exe`) and the program it installs (`paginaut.exe`), for each
release on [GitHub releases](https://github.com/adityapahurkar9/paginaut/releases). Windows shows **SignPath
Foundation** as their publisher.

Only what this project builds from its own source is signed. Both files are built from this repository by the
[Windows release workflow](.github/workflows/release-windows.yml) on GitHub's own runners, from the commit the
release is made from, and sent to SignPath from there; nothing built on a personal computer is signed. Third-party
code built into Paginaut (MuPDF, Tesseract, and the libraries listed in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)) is compiled from its published source in that same build.

Linux and Android builds aren't covered by this certificate: Linux packages are signed with Paginaut's update key,
and Android apps with the keys listed in [README.md](README.md#official-builds).

## Team and roles

- Committers and reviewers: [adityapahurkar9](https://github.com/adityapahurkar9) (the project's only maintainer;
  changes from anyone else are reviewed before they are merged)
- Approvers (each signing request is approved by hand): [adityapahurkar9](https://github.com/adityapahurkar9)

Everyone with these roles uses two-factor sign-in on GitHub and on SignPath.

## Privacy

This program will not transfer any information to other networked systems unless specifically requested by the
user or the person installing or operating it, with one exception you can turn off: once a day it asks GitHub
whether a newer version exists (Settings › About › Check automatically once a day). That request carries nothing
about you or your files. See the [privacy policy](PRIVACY.md) for everything Paginaut connects to and why.

## Reporting a problem

If you find a signed file that you think wasn't made by this project, or one that does something it shouldn't,
open an issue at [github.com/adityapahurkar9/paginaut/issues](https://github.com/adityapahurkar9/paginaut/issues) or
write to SignPath Foundation (see [signpath.org](https://signpath.org)).
