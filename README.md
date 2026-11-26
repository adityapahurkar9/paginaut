# Paginaut

Free, open-source PDF reader and editor for Windows and Linux, with an offline reader for Android. Opens PDF, EPUB, comics, XPS and Markdown.

Website: [adityapahurkar9.github.io/paginaut](https://adityapahurkar9.github.io/paginaut/)

## Official builds

Paginaut is only published in these places:

- **Google Play:** [app.paginaut.reader](https://play.google.com/store/apps/details?id=app.paginaut.reader)
  (in closed testing for now: the page opens only for testers until it's released)
- **GitHub releases:** [github.com/adityapahurkar9/paginaut/releases](https://github.com/adityapahurkar9/paginaut/releases)
  (Windows installer, Linux .deb and AppImage, Android APKs)

A copy from anywhere else may have been changed. Before installing an Android APK you got elsewhere, check that it is
signed by Paginaut's key. Google Play signs the app with the same key as the APKs on GitHub, so both have this
certificate:

SHA-256 fingerprint: `7A:27:CE:50:A9:72:C0:42:A6:1A:8E:93:5A:1B:1E:E4:95:9D:7B:02:34:92:7D:A6:00:1C:6D:2C:11:16:01:96`

To check an APK, run `apksigner verify --print-certs Paginaut.apk` (Android SDK build-tools) and compare the
`SHA-256 digest` line. Because the key is the same, a copy installed from Google Play can be updated from a newer
GitHub APK, and the other way round.

The desktop app installs only updates signed with Paginaut's update key, so its built-in updater can't be handed a
changed copy.

A copy that asks Android for more permissions than Paginaut uses (the internet, for one: official builds have no
internet access) refuses to start and says so.

## Name and logo

The source code is free under the [GNU AGPL 3.0 or later](https://www.gnu.org/licenses/agpl-3.0.html): you may
copy, change and share it under that licence. The name **Paginaut** and its logo are not part of that grant
(AGPL section 7(e)). If you publish a changed version, give it a different name and icon, and a different Android
package name, so nobody mistakes it for an official build.
