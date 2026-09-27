#!/usr/bin/env bash
# Build HydatekOS Link for Android without the Android SDK:
# javac against the Android framework classes, dx to dex, a generated binary
# manifest, and apksig for v2 signing. Needs a JDK 11+ and Python 3.
#
#   companion/android/build.sh   ->  companion/android/build/hydatek-link.apk
#
# Downloads (once, from Maven Central): Robolectric's android-all 14 jar
# (framework classes, ~140 MB), dalvik-dx 16.0.1, apksig 2.3.0.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
B="$HERE/build"
DEPS="$B/deps"
M2=https://repo.maven.apache.org/maven2
mkdir -p "$DEPS"
fetch() { [ -s "$DEPS/$2" ] || { echo "downloading $2"; curl -fsSL -o "$DEPS/$2" "$M2/$1"; }; }
fetch org/robolectric/android-all/14-robolectric-10818077/android-all-14-robolectric-10818077.jar android-all.jar
fetch com/jakewharton/android/repackaged/dalvik-dx/16.0.1/dalvik-dx-16.0.1.jar dx.jar
fetch com/android/tools/build/apksig/2.3.0/apksig-2.3.0.jar apksig.jar

rm -rf "$B/classes" "$B/tools" "$B/apk" && mkdir -p "$B/classes" "$B/tools" "$B/apk"
echo "compiling"
javac --release 8 -Xlint:-options -nowarn -cp "$DEPS/android-all.jar" -d "$B/classes" $(find "$HERE/src" -name '*.java')
echo "dexing"
java -cp "$DEPS/dx.jar" com.android.dx.command.Main --dex --min-sdk-version=26 --output="$B/apk/classes.dex" "$B/classes"
echo "manifest"
python3 "$HERE/tools/manifest.py" "$B/apk/AndroidManifest.xml"
(cd "$B/apk" && rm -f ../unsigned.apk && python3 -c "
import zipfile
with zipfile.ZipFile('../unsigned.apk', 'w', zipfile.ZIP_DEFLATED) as z:
    z.write('AndroidManifest.xml'); z.write('classes.dex')
")
KS="${HYDATEK_KEYSTORE:-$B/debug.p12}"
if [ ! -f "$KS" ]; then
  echo "creating a local signing key ($KS)"
  keytool -genkeypair -keystore "$KS" -storetype PKCS12 -storepass android -keypass android -alias hydatek \
    -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=HydatekOS Link" >/dev/null 2>&1
fi
javac -nowarn -cp "$DEPS/apksig.jar" -d "$B/tools" "$HERE/tools/Sign.java"
# apksig 2.3.0 predates the module system; its v1 signer class (loaded even
# though v1 is off) touches a JDK-internal X.509 class.
java --add-exports java.base/sun.security.x509=ALL-UNNAMED -cp "$B/tools:$DEPS/apksig.jar" Sign "$B/unsigned.apk" "$B/hydatek-link.apk" "$KS" "${HYDATEK_KEYSTORE_PASS:-android}"
ls -l "$B/hydatek-link.apk"
