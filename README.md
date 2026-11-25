# Paginaut

Free, open-source PDF reader and editor for Windows and Linux, with an offline reader for Android. Opens PDF, EPUB, comics, XPS and Markdown.

Website: [adityapahurkar9.github.io/paginaut](https://adityapahurkar9.github.io/paginaut/)

## Official builds

Paginaut is only published in these places:

- **Google Play:** [app.paginaut.reader](https://play.google.com/store/apps/details?id=app.paginaut.reader)
- **GitHub releases:** [github.com/adityapahurkar9/paginaut/releases](https://github.com/adityapahurkar9/paginaut/releases)
  (Windows installer, Linux .deb and AppImage, Android APKs)

A copy from anywhere else may have been changed. Before installing an Android APK you got elsewhere, check that it is
signed by one of these certificates:

| Where it comes from | SHA-256 fingerprint of the signing certificate |
| --- | --- |
| Google Play | `PLAY_APP_SIGNING_FINGERPRINT` |
| GitHub releases (APKs) | `7A:27:CE:50:A9:72:C0:42:A6:1A:8E:93:5A:1B:1E:E4:95:9D:7B:02:34:92:7D:A6:00:1C:6D:2C:11:16:01:96` |

To check an APK, run `apksigner verify --print-certs Paginaut.apk` (Android SDK build-tools) and compare the
`SHA-256 digest` line. Android refuses an update signed with a different key, so a copy installed from Google Play
can't be updated from a GitHub APK, or the other way round: uninstall one before installing the other.

The desktop app installs only updates signed with Paginaut's update key, so its built-in updater can't be handed a
changed copy.

Code signing for Windows: free code signing provided by [SignPath.io](https://about.signpath.io), certificate by
[SignPath Foundation](https://signpath.org), is being set up; releases before it are unsigned, and Windows may warn
before installing them. Signed releases name SignPath Foundation as the publisher. See the
[code signing policy](CODE_SIGNING_POLICY.md) for what is signed, how, and by whom.

A copy that asks Android for more permissions than Paginaut uses (the internet, for one: official builds have no
internet access) refuses to start and says so.

## Name and logo

The source code is free under the [GNU AGPL 3.0 or later](https://www.gnu.org/licenses/agpl-3.0.html): you may
copy, change and share it under that licence. The name **Paginaut** and its logo are not part of that grant
(AGPL section 7(e)). If you publish a changed version, give it a different name and icon, and a different Android
package name, so nobody mistakes it for an official build.
