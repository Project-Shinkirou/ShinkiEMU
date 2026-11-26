#!/usr/bin/env bash
# Run the device-test build with a disposable Keychain; never export credentials to Cargo.
set -euo pipefail
umask 077

: "${RUNNER_TEMP:?needs the hosted macOS runner}"
: "${SHINKI_EMU_IOS_CERTIFICATE_BASE64:?missing iOS Development certificate secret}"
: "${SHINKI_EMU_IOS_CERTIFICATE_PASSWORD:?missing iOS Development certificate password secret}"
: "${SHINKI_EMU_IOS_PROVISIONING_PROFILE_BASE64:?missing iOS development profile secret}"
: "${SHINKI_EMU_IOS_ENTITLEMENTS_BASE64:?missing iOS development entitlements secret}"

work=$(mktemp -d "$RUNNER_TEMP/shinki-ios-signing.XXXXXX")
keychain="$work/build.keychain-db"
cleanup() {
  security delete-keychain "$keychain" >/dev/null 2>&1 || true
  rm -rf -- "$work"
}
trap cleanup EXIT

printf %s "$SHINKI_EMU_IOS_CERTIFICATE_BASE64" | base64 -D > "$work/certificate.p12"
printf %s "$SHINKI_EMU_IOS_PROVISIONING_PROFILE_BASE64" | base64 -D > "$work/development.mobileprovision"
printf %s "$SHINKI_EMU_IOS_ENTITLEMENTS_BASE64" | base64 -D > "$work/entitlements.plist"
unset SHINKI_EMU_IOS_CERTIFICATE_BASE64 SHINKI_EMU_IOS_PROVISIONING_PROFILE_BASE64 SHINKI_EMU_IOS_ENTITLEMENTS_BASE64

password=$(openssl rand -hex 32)
security create-keychain -p "$password" "$keychain" >/dev/null
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
if ! security import "$work/certificate.p12" -k "$keychain" -f pkcs12 \
  -P "$SHINKI_EMU_IOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null 2>&1; then
  echo 'Cannot import the iOS Development certificate and private key' >&2
  exit 1
fi
unset SHINKI_EMU_IOS_CERTIFICATE_PASSWORD
rm -f -- "$work/certificate.p12"
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "$keychain" >/dev/null
unset password

identity=$(security find-identity -v -p codesigning "$keychain" | awk '
  /"(Apple Development|iPhone Developer):/ { print $2 }
')
if ! [[ "$identity" =~ ^[[:xdigit:]]{40}$ ]]; then
  echo 'The disposable keychain must contain exactly one valid Apple Development signing identity' >&2
  exit 1
fi
printf '::add-mask::%s\n' "$identity"
export SHINKI_EMU_IOS_SIGN_IDENTITY="$identity"
export SHINKI_EMU_IOS_KEYCHAIN="$keychain"
export SHINKI_EMU_IOS_PROVISIONING_PROFILE="$work/development.mobileprovision"
export SHINKI_EMU_IOS_ENTITLEMENTS="$work/entitlements.plist"
"$@"
