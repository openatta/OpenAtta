# Third-Party Notices

AttaOS is licensed under the Apache License, Version 2.0. This file documents
all third-party dependencies used by this project, their licenses, and any
required attributions.

**Total dependencies: 784**

## License Compatibility Summary

| Category | Count | Compatible with Apache-2.0 |
|----------|-------|---------------------------|
| Apache-2.0 OR MIT (dual) | 441 | Yes (choose either) |
| Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT | 37 | Yes |
| Apache-2.0 OR MIT OR Zlib | 15 | Yes |
| MIT OR Unlicense | 7 | Yes |
| Other permissive dual/multi-license | 16 | Yes |
| Apache-2.0 (single) | 8 | Yes (same license) |
| Apache-2.0 WITH LLVM-exception | 35 | Yes |
| Apache-2.0 AND ISC | 1 | Yes |
| MIT (single) | 167 | Yes |
| ISC (single) | 3 | Yes |
| BSD-3-Clause (single) | 3 | Yes |
| Unicode-3.0 | 18 | Yes |
| Zlib | 1 | Yes |
| CDLA-Permissive-2.0 | 1 | Yes |
| AND-compound (all permissive) | 5 | Yes |
| MPL-2.0 (file-level copyleft) | 5 | Yes (see Section 8) |
| BSD-3-Clause OR GPL-2.0 (choose BSD) | 2 | Yes (BSD-3-Clause chosen) |
| Apache-2.0 OR LGPL-2.1-or-later OR MIT | 1 | Yes (Apache-2.0 chosen) |

**Result: No license conflicts found.** All 784 dependencies are compatible
with the Apache-2.0 license.

---

## 1. Apache-2.0 OR MIT (Dual Licensed)

For these dependencies, this project uses them under the **Apache-2.0** license.

<details>
<summary>441 dependencies (click to expand)</summary>

addr2line 0.24.2, aead 0.5.2, aes 0.8.4, aes-gcm 0.10.3, ahash 0.8.12,
allocator-api2 0.2.21, android_system_properties 0.1.5, anes 0.1.6,
anstream 0.6.21, anstyle 1.0.13, anstyle-parse 0.2.7, anstyle-query 1.1.5,
anstyle-wincon 3.0.11, anyhow 1.0.102, arbitrary 1.4.2, async-trait 0.1.89,
atomic-waker 1.1.2, autocfg 1.5.0, base64 0.21.7, base64 0.22.1,
base64ct 1.8.3, bitflags 1.3.2, bitflags 2.11.0, block-buffer 0.10.4,
bumpalo 3.20.2, camino 1.2.2, cargo-platform 0.1.9, cargo_toml 0.22.3,
cast 0.3.0, cc 1.2.56, cesu8 1.1.0, cfg-expr 0.15.8, cfg-if 1.0.4,
chrono 0.4.44, cipher 0.4.4, clap 4.5.60, clap_builder 4.5.60,
clap_derive 4.5.55, clap_lex 1.0.0, cobs 0.3.0, colorchoice 1.0.4,
concurrent-queue 2.5.0, const-oid 0.9.6, cookie 0.18.1,
core-foundation 0.10.1, core-foundation 0.9.4, core-foundation-sys 0.8.7,
core-graphics 0.25.0, core-graphics-types 0.2.0, cpp_demangle 0.4.5,
cpufeatures 0.2.17, crc 3.4.0, crc-catalog 2.4.0, crc32fast 1.5.0,
criterion 0.5.1, criterion-plot 0.5.0, crossbeam-channel 0.5.15,
crossbeam-deque 0.8.6, crossbeam-epoch 0.9.18, crossbeam-queue 0.3.12,
crossbeam-utils 0.8.21, crypto-common 0.1.7, ctor 0.2.9, ctr 0.9.2,
der 0.7.10, deranged 0.3.11, derive_arbitrary 1.4.2, digest 0.10.7,
directories-next 2.0.0, dirs 4.0.0, dirs 6.0.0, dirs-sys 0.3.7,
dirs-sys 0.5.0, dirs-sys-next 0.1.2, displaydoc 0.2.5, dtoa 1.0.11,
dyn-clone 1.0.20, either 1.15.0, embed_plist 1.2.2, embedded-io 0.4.0,
embedded-io 0.6.1, equivalent 1.0.2, erased-serde 0.4.10, errno 0.3.14,
etcetera 0.8.0, event-listener 5.4.1, fallible-iterator 0.3.0,
fastrand 2.3.0, fd-lock 4.0.4, fdeflate 0.3.7, field-offset 0.3.6,
filetime 0.2.27, find-msvc-tools 0.1.9, flate2 1.1.9, flume 0.11.1,
fnv 1.0.7, foreign-types 0.3.2, foreign-types 0.5.0,
foreign-types-macros 0.2.3, foreign-types-shared 0.1.1,
foreign-types-shared 0.3.1, form_urlencoded 1.2.2, futf 0.1.5,
futures 0.3.32, futures-channel 0.3.32, futures-core 0.3.32,
futures-executor 0.3.32, futures-intrusive 0.5.0, futures-io 0.3.32,
futures-macro 0.3.32, futures-sink 0.3.32, futures-task 0.3.32,
futures-util 0.3.32, fxhash 0.2.1, fxprof-processed-profile 0.6.0,
getrandom 0.1.16, getrandom 0.2.17, getrandom 0.3.4, getrandom 0.4.1,
ghash 0.5.1, gimli 0.31.1, glob 0.3.3, half 2.7.1, hashbrown 0.12.3,
hashbrown 0.14.5, hashbrown 0.15.5, hashbrown 0.16.1, hashlink 0.10.0,
heck 0.4.1, heck 0.5.0, hermit-abi 0.5.2, hex 0.4.3, hkdf 0.12.4,
hmac 0.12.1, home 0.5.11, html5ever 0.29.1, http 1.4.0, httparse 1.10.1,
httpdate 1.0.3, hyper-tls 0.6.0, iana-time-zone 0.1.65,
iana-time-zone-haiku 0.1.2, id-arena 2.3.0, ident_case 1.0.1, idna 1.1.0,
idna_adapter 1.2.1, indexmap 1.9.3, indexmap 2.13.0, inout 0.1.4,
ipnet 2.12.0, iri-string 0.7.10, is_terminal_polyfill 1.70.2,
itertools 0.10.5, itertools 0.12.1, itoa 1.0.17, jni 0.21.1, jni-sys 0.3.0,
jobserver 0.1.34, js-sys 0.3.91, json-patch 3.0.1, jsonptr 0.6.3,
keyboard-types 0.7.0, lazy_static 1.5.0, leb128 0.2.5, leb128fmt 0.1.0,
libappindicator 0.9.0, libappindicator-sys 0.9.0, libc 0.2.182,
lock_api 0.4.14, log 0.4.29, mac 0.1.1, markup5ever 0.14.1,
match_token 0.1.0, maybe-owned 0.3.4, md-5 0.10.6, memfd 0.6.5,
mime 0.3.17, muda 0.15.3, muda 0.17.1, native-tls 0.2.18, ndk 0.9.0,
ndk-context 0.1.1, ndk-sys 0.6.0, nodrop 0.1.14, ntapi 0.4.3,
num-bigint-dig 0.8.6, num-conv 0.1.0, num-integer 0.1.46, num-iter 0.1.45,
num-traits 0.2.19, object 0.36.7, object 0.37.3, once_cell 1.21.3,
once_cell_polyfill 1.70.2, opaque-debug 0.3.1, openssl-macros 0.1.1,
openssl-probe 0.2.1, osakit 0.3.1, parking 2.2.1, parking_lot 0.12.5,
parking_lot_core 0.9.12, paste 1.0.15, pathdiff 0.2.3, pem-rfc7468 0.7.0,
percent-encoding 2.3.2, pin-project-lite 0.2.17, pin-utils 0.1.0,
pkcs1 0.7.5, pkcs8 0.10.2, pkg-config 0.3.32, plain 0.2.3, png 0.17.16,
polyval 0.6.2, postcard 1.1.3, powerfmt 0.2.0, ppv-lite86 0.2.21,
prettyplease 0.2.37, proc-macro-crate 1.3.1, proc-macro-crate 2.0.0,
proc-macro-error 1.0.4, proc-macro-error-attr 1.0.4,
proc-macro-hack 0.5.20, proc-macro2 1.0.106, psm 0.1.30, quote 1.0.44,
rand 0.7.3, rand 0.8.5, rand_chacha 0.2.2, rand_chacha 0.3.1,
rand_core 0.5.1, rand_core 0.6.4, rand_hc 0.2.0, rand_pcg 0.2.1,
rayon 1.11.0, rayon-core 1.13.0, ref-cast 1.0.25, ref-cast-impl 1.0.25,
regex 1.12.3, regex-automata 0.4.14, regex-syntax 0.8.10, reqwest 0.12.28,
reqwest 0.13.2, rsa 0.9.10, rustc-demangle 0.1.27, rustc-hash 2.1.1,
rustc_version 0.4.1, rustls-pki-types 1.14.0, rustls-platform-verifier 0.6.2,
rustls-platform-verifier-android 0.1.1, rustversion 1.0.22,
scopeguard 1.2.0, secrecy 0.10.3, security-framework 3.7.0,
security-framework-sys 2.17.0, semver 1.0.27, serde 1.0.228,
serde-untagged 0.1.9, serde_core 1.0.228, serde_derive 1.0.228,
serde_derive_internals 0.29.1, serde_json 1.0.149,
serde_path_to_error 0.1.20, serde_repr 0.1.20, serde_spanned 0.6.9,
serde_spanned 1.0.4, serde_urlencoded 0.7.1, serde_with 3.17.0,
serde_with_macros 3.17.0, serde_yaml 0.9.34, serialize-to-javascript 0.1.2,
serialize-to-javascript-impl 0.1.2, servo_arc 0.2.0, sha1 0.10.6,
sha2 0.10.9, shellexpand 2.1.2, shlex 1.3.0, signal-hook 0.3.18,
signal-hook-registry 1.4.8, signature 2.2.0, siphasher 0.3.11,
siphasher 1.0.2, smallvec 1.15.1, socket2 0.6.2, softbuffer 0.4.8,
spki 0.7.3, sptr 0.3.2, sqlx 0.8.6, sqlx-core 0.8.6, sqlx-macros 0.8.6,
sqlx-macros-core 0.8.6, sqlx-mysql 0.8.6, sqlx-postgres 0.8.6,
sqlx-sqlite 0.8.6, stable_deref_trait 1.2.1, string_cache 0.8.9,
string_cache_codegen 0.5.4, stringprep 0.1.5, swift-rs 1.0.7, syn 1.0.109,
syn 2.0.117, system-configuration 0.7.0, system-configuration-sys 0.6.0,
system-deps 6.2.2, tao-macros 0.1.3, tar 0.4.44, tauri 2.10.3,
tauri-build 2.5.6, tauri-codegen 2.5.5, tauri-macros 2.5.5,
tauri-plugin 2.5.4, tauri-plugin-shell 2.3.5, tauri-plugin-updater 2.10.0,
tauri-runtime 2.10.1, tauri-runtime-wry 2.10.1, tauri-utils 2.8.3,
tempfile 3.26.0, tendril 0.4.3, thiserror 1.0.69, thiserror 2.0.18,
thiserror-impl 1.0.69, thiserror-impl 2.0.18, thread_local 1.1.9,
time 0.3.36, time-core 0.1.2, time-macros 0.2.18, tinytemplate 1.2.1,
tokio-rustls 0.26.4, toml 0.8.23, toml 0.9.12, toml_datetime 0.6.11,
toml_datetime 0.7.5, toml_edit 0.19.15, toml_edit 0.20.7, toml_edit 0.22.27,
toml_parser 1.0.9, toml_write 0.1.2, toml_writer 1.0.6, tray-icon 0.19.3,
tray-icon 0.21.3, tungstenite 0.24.0, typeid 1.0.3, typenum 1.19.0,
unic-char-property 0.9.0, unic-char-range 0.9.0, unic-common 0.9.0,
unic-ucd-ident 0.9.0, unic-ucd-version 0.9.0, unicase 2.9.0,
unicode-bidi 0.3.18, unicode-normalization 0.1.25, unicode-properties 0.1.4,
unicode-segmentation 1.12.0, unicode-width 0.2.2, unicode-xid 0.2.6,
universal-hash 0.5.1, url 2.5.8, utf-8 0.7.6, utf8_iter 1.0.4,
utf8parse 0.2.2, uuid 1.21.0, vcpkg 0.2.15, version_check 0.9.5,
wasm-bindgen 0.2.114, wasm-bindgen-futures 0.4.64,
wasm-bindgen-macro 0.2.114, wasm-bindgen-macro-support 0.2.114,
wasm-bindgen-shared 0.2.114, wasm-streams 0.4.2, wasm-streams 0.5.0,
web-sys 0.3.91, winapi 0.3.9, winapi-i686-pc-windows-gnu 0.4.0,
winapi-x86_64-pc-windows-gnu 0.4.0, window-vibrancy 0.6.0, windows 0.57.0,
windows 0.61.3, windows-collections 0.2.0, windows-core 0.57.0,
windows-core 0.61.2, windows-core 0.62.2, windows-future 0.2.1,
windows-implement 0.57.0, windows-implement 0.60.2, windows-interface 0.57.0,
windows-interface 0.59.3, windows-link 0.1.3, windows-link 0.2.1,
windows-numerics 0.2.0, windows-registry 0.6.1, windows-result 0.1.2,
windows-result 0.3.4, windows-result 0.4.1, windows-strings 0.4.2,
windows-strings 0.5.1, windows-sys 0.45.0, windows-sys 0.48.0,
windows-sys 0.52.0, windows-sys 0.59.0, windows-sys 0.60.2,
windows-sys 0.61.2, windows-targets 0.42.2, windows-targets 0.48.5,
windows-targets 0.52.6, windows-targets 0.53.5, windows-threading 0.1.0,
windows-version 0.1.7, windows_aarch64_gnullvm (0.42-0.53),
windows_aarch64_msvc (0.42-0.53), windows_i686_gnu (0.42-0.53),
windows_i686_gnullvm (0.52-0.53), windows_i686_msvc (0.42-0.53),
windows_x86_64_gnu (0.42-0.53), windows_x86_64_gnullvm (0.42-0.53),
windows_x86_64_msvc (0.42-0.53), wry 0.54.2, xattr 1.6.1, zeroize 1.8.2,
zstd-safe 7.2.4, zstd-sys 2.0.16

</details>

## 2. Apache-2.0 (Single License)

| Crate | Version |
|-------|---------|
| ciborium | 0.2.2 |
| ciborium-io | 0.2.2 |
| ciborium-ll | 0.2.2 |
| debugid | 0.8.0 |
| openssl | 0.10.75 |
| sync_wrapper | 1.0.2 |
| tao | 0.34.6 |
| witx | 0.9.1 |

## 4. MIT (Single License)

The MIT License requires inclusion of the copyright notice and license text.
The following dependencies are used under the MIT License.

<details>
<summary>167 dependencies (click to expand)</summary>

async-stream 0.3.6, async-stream-impl 0.3.6, atk 0.18.2, atk-sys 0.18.2,
atoi 2.0.0, axum 0.7.9, axum-core 0.4.5, block2 0.5.1, block2 0.6.2,
bytes 1.11.1, cairo-rs 0.18.5, cairo-sys-rs 0.18.2, cargo_metadata 0.19.2,
cfb 0.7.3, combine 4.6.7, convert_case 0.4.0, crunchy 0.2.4,
darling 0.21.3, darling_core 0.21.3, darling_macro 0.21.3,
data-encoding 2.10.0, derive_more 0.99.20, dlopen2 0.8.2,
dlopen2_derive 0.4.3, dotenvy 0.15.7, embed-resource 3.0.6, gdk 0.18.2,
gdk-pixbuf 0.18.5, gdk-pixbuf-sys 0.18.0, gdk-sys 0.18.2,
gdkwayland-sys 0.18.2, gdkx11 0.18.2, gdkx11-sys 0.18.2,
generic-array 0.14.7, gio 0.18.4, gio-sys 0.18.1, glib 0.18.5,
glib-macros 0.18.5, glib-sys 0.18.1, gobject-sys 0.18.0, gtk 0.18.2,
gtk-sys 0.18.2, gtk3-macros 0.18.2, h2 0.4.13, hostname 0.4.2,
http-body 1.0.1, http-body-util 0.1.3, hyper 1.8.1, hyper-util 0.1.20,
ico 0.5.0, infer 0.19.0, is-docker 0.2.0, is-terminal 0.4.17, is-wsl 0.4.0,
javascriptcore-rs 1.1.2, javascriptcore-rs-sys 1.1.1,
kuchikiki 0.8.8-speedreader, libm 0.2.16, libredox 0.1.14,
libsqlite3-sys 0.30.1, libxdo 0.6.0, libxdo-sys 0.11.0, matchers 0.2.0,
matches 0.1.10, memoffset 0.9.1, mime_guess 2.0.5, minisign-verify 0.2.5,
mio 1.1.1, new_debug_unreachable 1.0.6, nu-ansi-term 0.50.3,
objc-sys 0.3.5, objc2 0.5.2, objc2 0.6.4, objc2-app-kit 0.2.2,
objc2-core-data 0.2.2, objc2-core-image 0.2.2, objc2-encode 4.1.0,
objc2-foundation 0.2.2, objc2-foundation 0.3.2, objc2-metal 0.2.2,
objc2-quartz-core 0.2.2, oorandom 11.1.5, open 5.3.3, openssl-sys 0.9.111,
os_pipe 1.2.3, pango 0.18.3, pango-sys 0.18.0, phf 0.10.1, phf 0.11.3,
phf 0.8.0, phf_codegen 0.11.3, phf_codegen 0.8.0, phf_generator 0.10.0,
phf_generator 0.11.3, phf_generator 0.8.0, phf_macros 0.10.0,
phf_macros 0.11.3, phf_shared 0.10.0, phf_shared 0.11.3, phf_shared 0.8.0,
plist 1.8.0, plotters 0.3.7, plotters-backend 0.3.7, plotters-svg 0.3.7,
precomputed-hash 0.1.1, quick-xml 0.38.4, redox_syscall 0.5.18,
redox_syscall 0.7.3, redox_users 0.4.6, redox_users 0.5.2,
rust-embed 8.11.0, rust-embed-impl 8.11.0, rust-embed-utils 8.11.0,
schannel 0.1.28, schemars 0.8.22, schemars 0.9.0, schemars 1.2.1,
schemars_derive 0.8.22, sharded-slab 0.1.7, shared_child 1.1.1,
sigchld 0.2.4, simd-adler32 0.3.8, slab 0.4.12, slice-group-by 0.3.1,
soup3 0.5.0, soup3-sys 0.5.0, spin 0.9.8, strsim 0.11.1,
synstructure 0.13.2, sysinfo 0.33.1, tauri-winres 0.3.5, tokio 1.50.0,
tokio-macros 2.6.1, tokio-native-tls 0.3.1, tokio-stream 0.1.18,
tokio-tungstenite 0.24.0, tokio-util 0.7.18, tower 0.5.3,
tower-http 0.6.8, tower-layer 0.3.3, tower-service 0.3.3, tracing 0.1.44,
tracing-attributes 0.1.31, tracing-core 0.1.36, tracing-log 0.2.0,
tracing-subscriber 0.3.22, try-lock 0.2.5, unsafe-libyaml 0.2.11,
urlpattern 0.3.0, valuable 0.1.1, version-compare 0.2.1, vswhom 0.1.0,
vswhom-sys 0.1.3, want 0.3.1, webkit2gtk 2.0.2, webkit2gtk-sys 2.0.2,
webview2-com 0.38.2, webview2-com-macros 0.8.1, webview2-com-sys 0.38.2,
winnow 0.5.40, winnow 0.7.14, winreg 0.55.0, x11 2.21.0, x11-dl 2.21.0,
zip 4.6.1, zmij 1.0.21, zstd 0.13.3

</details>

## 5. ISC License

| Crate | Version | Copyright |
|-------|---------|-----------|
| libloading | 0.7.4 | Copyright (c) Simonas Kazlauskas |
| rustls-webpki | 0.103.9 | Copyright (c) the rustls-webpki contributors |
| untrusted | 0.9.0 | Copyright (c) Brian Smith |

## 6. BSD-3-Clause

| Crate | Version | Copyright |
|-------|---------|-----------|
| alloc-no-stdlib | 2.0.4 | Copyright (c) Brookhaven Science Associates |
| alloc-stdlib | 0.2.2 | Copyright (c) Brookhaven Science Associates |
| subtle | 2.6.1 | Copyright (c) 2016-2017 Isis Agora Lovecruft, Henry de Valence |

## 7. Apache-2.0 AND ISC (Dual Obligation)

| Crate | Version | Note |
|-------|---------|------|
| ring | 0.17.14 | Copyright (c) the ring contributors. Both Apache-2.0 AND ISC apply simultaneously. Both are permissive licenses. |

## 8. MPL-2.0 (Mozilla Public License 2.0) -- Special Notice

The following dependencies are licensed under MPL-2.0, a **file-level copyleft**
license. They are brought in transitively through Tauri's web rendering stack
(Servo-derived CSS parsing). MPL-2.0 is compatible with Apache-2.0 when used
as compiled dependencies without source modification.

**If you modify the source files of these crates, those modifications must be
released under MPL-2.0.**

| Crate | Version | Source |
|-------|---------|--------|
| cssparser | 0.29.6 | Servo project (Mozilla) |
| cssparser-macros | 0.6.1 | Servo project (Mozilla) |
| dtoa-short | 0.3.5 | Servo project (Mozilla) |
| option-ext | 0.2.0 | Used by `dirs` crate |
| selectors | 0.24.0 | Servo project (Mozilla) |

The source code for these crates is available at:
- https://crates.io/crates/cssparser
- https://crates.io/crates/cssparser-macros
- https://crates.io/crates/dtoa-short
- https://crates.io/crates/option-ext
- https://crates.io/crates/selectors

## 9. Unicode-3.0

The following ICU4X-related crates are licensed under the Unicode License
Agreement (Unicode-3.0), a permissive license compatible with Apache-2.0.

| Crate | Version |
|-------|---------|
| icu_collections | 2.1.1 |
| icu_locale_core | 2.1.1 |
| icu_normalizer | 2.1.1 |
| icu_normalizer_data | 2.1.1 |
| icu_properties | 2.1.2 |
| icu_properties_data | 2.1.2 |
| icu_provider | 2.1.1 |
| litemap | 0.8.1 |
| potential_utf | 0.1.4 |
| tinystr | 0.8.2 |
| writeable | 0.6.2 |
| yoke | 0.8.1 |
| yoke-derive | 0.8.1 |
| zerofrom | 0.1.6 |
| zerofrom-derive | 0.1.6 |
| zerotrie | 0.2.3 |
| zerovec | 0.11.5 |
| zerovec-derive | 0.11.2 |

## 10. AND-Compound Licenses (All Permissive)

These use AND (both apply simultaneously). All components are permissive.

| Crate | Version | License |
|-------|---------|---------|
| brotli | 8.0.2 | BSD-3-Clause AND MIT |
| dpi | 0.1.2 | Apache-2.0 AND MIT |
| matchit | 0.7.3 | BSD-3-Clause AND MIT |
| encoding_rs | 0.8.35 | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| unicode-ident | 1.0.24 | (Apache-2.0 OR MIT) AND Unicode-3.0 |

## 11. Other Permissive Licenses

| Crate | Version | License | Note |
|-------|---------|---------|------|
| adler2 | 2.0.1 | 0BSD OR Apache-2.0 OR MIT | 0BSD is a public-domain equivalent |
| brotli-decompressor | 5.0.0 | BSD-3-Clause OR MIT | Choose MIT |
| dunce | 1.0.5 | Apache-2.0 OR CC0-1.0 OR MIT-0 | Choose Apache-2.0 |
| foldhash | 0.1.5 | Zlib | Permissive |
| hyper-rustls | 0.27.7 | Apache-2.0 OR ISC OR MIT | Choose Apache-2.0 |
| mach2 | 0.4.3 | Apache-2.0 OR BSD-2-Clause OR MIT | Choose Apache-2.0 |
| num_enum | 0.7.5 | Apache-2.0 OR BSD-3-Clause OR MIT | Choose Apache-2.0 |
| num_enum_derive | 0.7.5 | Apache-2.0 OR BSD-3-Clause OR MIT | Choose Apache-2.0 |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | Choose Apache-2.0 |
| rustls | 0.23.37 | Apache-2.0 OR ISC OR MIT | Choose Apache-2.0 |
| rustls-native-certs | 0.8.3 | Apache-2.0 OR ISC OR MIT | Choose Apache-2.0 |
| wasite | 0.1.0 | Apache-2.0 OR BSL-1.0 OR MIT | Choose Apache-2.0 |
| whoami | 1.6.1 | Apache-2.0 OR BSL-1.0 OR MIT | Choose Apache-2.0 |
| zerocopy | 0.8.40 | Apache-2.0 OR BSD-2-Clause OR MIT | Choose Apache-2.0 |
| zerocopy-derive | 0.8.40 | Apache-2.0 OR BSD-2-Clause OR MIT | Choose Apache-2.0 |
| webpki-root-certs | 1.0.6 | CDLA-Permissive-2.0 | Community Data License Agreement, Permissive 2.0 |

## 12. Dual-Licensed with Copyleft Option (Permissive Chosen)

For these crates, we explicitly choose the permissive license option.

| Crate | Version | License Options | **Chosen** |
|-------|---------|-----------------|------------|
| ittapi | 0.4.0 | BSD-3-Clause OR GPL-2.0 | **BSD-3-Clause** |
| ittapi-sys | 0.4.0 | BSD-3-Clause OR GPL-2.0 | **BSD-3-Clause** |
| r-efi | 5.3.0 | Apache-2.0 OR LGPL-2.1-or-later OR MIT | **Apache-2.0** |

## 13. MIT OR Unlicense

| Crate | Version |
|-------|---------|
| aho-corasick | 1.1.4 |
| byteorder | 1.5.0 |
| memchr | 2.8.0 |
| same-file | 1.0.6 |
| termcolor | 1.4.1 |
| walkdir | 2.5.0 |
| winapi-util | 0.1.11 |

---

## License Texts

### Apache License 2.0
See the `LICENSE` file in the root of this repository.

### MIT License
```
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### ISC License
```
Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

### BSD-3-Clause License
```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice,
   this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.
3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### MPL-2.0 (Mozilla Public License 2.0)
Full text: https://www.mozilla.org/en-US/MPL/2.0/

### Unicode License Agreement (Unicode-3.0)
Full text: https://www.unicode.org/license.txt

### CDLA-Permissive-2.0
Full text: https://cdla.dev/permissive-2-0/
