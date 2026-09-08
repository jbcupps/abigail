# The old builder targeted tauri-app and omitted the current offline product.
# Fail before changing source, installing tools, or building an obsolete installer.
param([Parameter(ValueFromRemainingArguments = $true)][object[]]$LegacyArguments)
throw 'This legacy release builder is retired. Use scripts/build_signed_installer.ps1 -Phase Prepare, then -Phase Check, then the private configure_esigner_credentials.ps1 flow. See docs/WINDOWS_SIGNING.md. No build was started.'
