# 第三方组件许可与使用说明


## 本说明的范围

MaiBot Manager TUI 自身遵循仓库中的 [AGPL-3.0](LICENSE)。
本说明用于标明独立外部程序、相关服务及 Rust 依赖的许可边界，各项目名称、商标及著作权归各自权利人所有；本管理器不因集成管理入口而取得上游或腾讯的背书。

管理器通过用户执行时的下载、外部进程或本地容器管理相关软件，不能据此将它们全部视为本项目 AGPL 许可的一部分。重新打包、镜像托管、分发修改版、商业使用与单纯调用已有程序，应分别核对适用条款。

## 独立外部软件

| 组件 | 官方来源与许可 | 使用边界 |
| --- | --- | --- |
| MaiBot | [官方仓库](https://github.com/Mai-with-u/MaiBot)、[GPL-3.0](https://github.com/Mai-with-u/MaiBot/blob/main/LICENSE)、[EULA](https://github.com/Mai-with-u/MaiBot/blob/main/EULA.md)、[隐私政策](https://github.com/Mai-with-u/MaiBot/blob/main/PRIVACY.md) | 主程序源代码以 GPLv3 为基础，另有许可声明的组件除外；当前 EULA 未见自动化部署须书面授权的限制。运行前须由用户阅读并同意适用条款；商业使用、交流参与者告知等义务见下文。 |
| NapCatQQ | [官方仓库](https://github.com/NapNeko/NapCatQQ)、[Limited Redistribution License for NapCat](https://github.com/NapNeko/NapCatQQ/blob/main/LICENSE)、[README](https://github.com/NapNeko/NapCatQQ#license) | 采用自定义许可，不应当作 MIT 等宽松许可。包含非商业、使用与再分发限制；未明确授予的权利须向主作者申请。 |
| LuckyLilliaBot / LLBot | [官方仓库](https://github.com/LLOneBot/LuckyLilliaBot)、[GPL-2.0](https://github.com/LLOneBot/LuckyLilliaBot/blob/main/LICENSE) | GPL 不限制程序运行；分发二进制或修改版时须按条款保留声明、提供许可和对应源码或适用的源码提供方式。随附第三方组件分别遵守其条款。 |
| SnowLuma | [源码许可](https://github.com/SnowLuma/SnowLuma/blob/v2.1.1/LICENSE)、[EULA v1.1](https://github.com/SnowLuma/SnowLuma/blob/v2.1.1/EULA.md)、[隐私政策](https://github.com/SnowLuma/SnowLuma/blob/v2.1.1/PRIVACY.md) | 源码可见非商业许可；随附原生组件为专有组件。自动化部署、并入第三方安装包或镜像、商业使用等须事先取得相应书面授权。 |
| MaiBot-SnowLuma-Adapter（统一 QQ 适配器） | [官方仓库及使用说明](https://github.com/Mai-with-u/MaiBot-SnowLuma-Adapter) | 与 SnowLuma 协议端是不同项目。核查时根目录未见独立 LICENSE；须以该项目的实际声明或维护者授权为准，不以本管理器或 SnowLuma 的许可替代。 |
| QQ / NTQQ 客户端及服务 | [QQ 软件许可及服务协议](https://ti.qq.com/agreement/index.html) | 属于腾讯软件与服务，第三方项目许可不代表腾讯授权。接入方式、自动化行为、账号和数据处理须另行遵守平台条款。 |

Git、uv、Python、Docker、screen、Homebrew 及所调用的安装脚本、系统软件包也分别遵循自身许可和服务条款。本仓库的许可不会改变这些工具的权利范围。实际下载的完整发行包、插件、镜像和工具可能含有额外组件，请保留并核对其随附声明。

## SnowLuma 的管理边界

本管理器不提供 SnowLuma 的自动安装、下载、镜像拉取、Compose / `.env` 生成或迁移、更新、容器重建，以及清空数据后重新初始化。用户需先按 [官方部署文档](https://snowluma.github.io/zh/docs/guide/deploy/) 自行部署、阅读并接受适用条款

`SNOWLUMA_ACCEPT_EULA` 和 `SNOWLUMA_ACCEPT_PRIVACY` 等协议确认参数，不等于著作权人针对自动化部署出具的书面授权。管理器不会代用户设置这些参数或作出授权承诺。

## 再分发与用户责任

- 保留第三方软件随附的版权、署名、许可和免责声明
- 自行打包第三方二进制、源代码或容器镜像前，核对再分发权利、对应源码义务及专有组件的单独限制
- 本项目免费或开源、用户点击同意、通过下载代理获取文件，均不会扩大上游许可范围。
- 第三方项目对运行的许可不等于腾讯平台接入许可。QQ 协议端运行可能涉及账号限制、消息与个人信息处理等独立风险

## Rust 编译依赖清单

以下清单来自本仓库 `Cargo.lock` 和 `cargo metadata --locked`，涵盖全部解析到的目标平台依赖及构建依赖；某个具体发行二进制不一定包含全部条目。许可表达式按 crate 发布元数据原样记录，`OR` 通常表示可按其中一种许可使用，`AND` 表示须同时满足；实际权利及版权信息以对应版本的源码包为准。

依赖更新时应重新核对本节，发布方应按实际包含的组件随发行物保留所需许可和版权声明。下列版本链接可查阅对应源码包；`option-ext` 的 MPL-2.0、`foldhash` 的 Zlib、`unicode-ident` 额外的 Unicode 条款等不能被本项目 AGPL 声明取代。

| Crate | 锁定版本 | 许可表达式 | 对应版本源码 |
| --- | --- | --- | --- |
| `aho-corasick` | `1.1.4` | `Unlicense OR MIT` | [crate](https://crates.io/crates/aho-corasick/1.1.4) |
| `allocator-api2` | `0.2.21` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/allocator-api2/0.2.21) |
| `anyhow` | `1.0.102` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/anyhow/1.0.102) |
| `bitflags` | `2.11.1` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/bitflags/2.11.1) |
| `block2` | `0.6.2` | `MIT` | [crate](https://crates.io/crates/block2/0.6.2) |
| `cassowary` | `0.3.0` | `MIT / Apache-2.0` | [crate](https://crates.io/crates/cassowary/0.3.0) |
| `castaway` | `0.2.4` | `MIT` | [crate](https://crates.io/crates/castaway/0.2.4) |
| `cfg-if` | `1.0.4` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/cfg-if/1.0.4) |
| `cfg_aliases` | `0.2.1` | `MIT` | [crate](https://crates.io/crates/cfg_aliases/0.2.1) |
| `compact_str` | `0.8.2` | `MIT` | [crate](https://crates.io/crates/compact_str/0.8.2) |
| `console` | `0.16.3` | `MIT` | [crate](https://crates.io/crates/console/0.16.3) |
| `convert_case` | `0.10.0` | `MIT` | [crate](https://crates.io/crates/convert_case/0.10.0) |
| `crossterm` | `0.28.1` | `MIT` | [crate](https://crates.io/crates/crossterm/0.28.1) |
| `crossterm` | `0.29.0` | `MIT` | [crate](https://crates.io/crates/crossterm/0.29.0) |
| `crossterm_winapi` | `0.9.1` | `MIT` | [crate](https://crates.io/crates/crossterm_winapi/0.9.1) |
| `ctrlc` | `3.5.2` | `MIT/Apache-2.0` | [crate](https://crates.io/crates/ctrlc/3.5.2) |
| `darling` | `0.23.0` | `MIT` | [crate](https://crates.io/crates/darling/0.23.0) |
| `darling_core` | `0.23.0` | `MIT` | [crate](https://crates.io/crates/darling_core/0.23.0) |
| `darling_macro` | `0.23.0` | `MIT` | [crate](https://crates.io/crates/darling_macro/0.23.0) |
| `derive_more` | `2.1.1` | `MIT` | [crate](https://crates.io/crates/derive_more/2.1.1) |
| `derive_more-impl` | `2.1.1` | `MIT` | [crate](https://crates.io/crates/derive_more-impl/2.1.1) |
| `dialoguer` | `0.12.0` | `MIT` | [crate](https://crates.io/crates/dialoguer/0.12.0) |
| `dirs` | `6.0.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/dirs/6.0.0) |
| `dirs-sys` | `0.5.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/dirs-sys/0.5.0) |
| `dispatch2` | `0.3.1` | `Zlib OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/dispatch2/0.3.1) |
| `document-features` | `0.2.12` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/document-features/0.2.12) |
| `either` | `1.16.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/either/1.16.0) |
| `encode_unicode` | `1.0.0` | `Apache-2.0 OR MIT` | [crate](https://crates.io/crates/encode_unicode/1.0.0) |
| `equivalent` | `1.0.2` | `Apache-2.0 OR MIT` | [crate](https://crates.io/crates/equivalent/1.0.2) |
| `errno` | `0.3.14` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/errno/0.3.14) |
| `fastrand` | `2.4.1` | `Apache-2.0 OR MIT` | [crate](https://crates.io/crates/fastrand/2.4.1) |
| `foldhash` | `0.1.5` | `Zlib` | [crate](https://crates.io/crates/foldhash/0.1.5) |
| `getrandom` | `0.2.17` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/getrandom/0.2.17) |
| `getrandom` | `0.4.2` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/getrandom/0.4.2) |
| `hashbrown` | `0.15.5` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/hashbrown/0.15.5) |
| `hashbrown` | `0.17.1` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/hashbrown/0.17.1) |
| `heck` | `0.5.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/heck/0.5.0) |
| `id-arena` | `2.3.0` | `MIT/Apache-2.0` | [crate](https://crates.io/crates/id-arena/2.3.0) |
| `ident_case` | `1.0.1` | `MIT/Apache-2.0` | [crate](https://crates.io/crates/ident_case/1.0.1) |
| `indexmap` | `2.14.0` | `Apache-2.0 OR MIT` | [crate](https://crates.io/crates/indexmap/2.14.0) |
| `indoc` | `2.0.7` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/indoc/2.0.7) |
| `instability` | `0.3.12` | `MIT` | [crate](https://crates.io/crates/instability/0.3.12) |
| `itertools` | `0.13.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/itertools/0.13.0) |
| `itoa` | `1.0.18` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/itoa/1.0.18) |
| `leb128fmt` | `0.1.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/leb128fmt/0.1.0) |
| `libc` | `0.2.186` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/libc/0.2.186) |
| `libredox` | `0.1.16` | `MIT` | [crate](https://crates.io/crates/libredox/0.1.16) |
| `linux-raw-sys` | `0.12.1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/linux-raw-sys/0.12.1) |
| `linux-raw-sys` | `0.4.15` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/linux-raw-sys/0.4.15) |
| `litrs` | `1.0.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/litrs/1.0.0) |
| `lock_api` | `0.4.14` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/lock_api/0.4.14) |
| `log` | `0.4.29` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/log/0.4.29) |
| `lru` | `0.12.5` | `MIT` | [crate](https://crates.io/crates/lru/0.12.5) |
| `memchr` | `2.8.0` | `Unlicense OR MIT` | [crate](https://crates.io/crates/memchr/2.8.0) |
| `mio` | `1.2.0` | `MIT` | [crate](https://crates.io/crates/mio/1.2.0) |
| `nix` | `0.31.3` | `MIT` | [crate](https://crates.io/crates/nix/0.31.3) |
| `objc2` | `0.6.4` | `MIT` | [crate](https://crates.io/crates/objc2/0.6.4) |
| `objc2-encode` | `4.1.0` | `MIT` | [crate](https://crates.io/crates/objc2-encode/4.1.0) |
| `once_cell` | `1.21.4` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/once_cell/1.21.4) |
| `option-ext` | `0.2.0` | `MPL-2.0` | [crate](https://crates.io/crates/option-ext/0.2.0) |
| `parking_lot` | `0.12.5` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/parking_lot/0.12.5) |
| `parking_lot_core` | `0.9.12` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/parking_lot_core/0.9.12) |
| `paste` | `1.0.15` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/paste/1.0.15) |
| `prettyplease` | `0.2.37` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/prettyplease/0.2.37) |
| `proc-macro2` | `1.0.106` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/proc-macro2/1.0.106) |
| `quote` | `1.0.45` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/quote/1.0.45) |
| `r-efi` | `6.0.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | [crate](https://crates.io/crates/r-efi/6.0.0) |
| `ratatui` | `0.29.0` | `MIT` | [crate](https://crates.io/crates/ratatui/0.29.0) |
| `redox_syscall` | `0.5.18` | `MIT` | [crate](https://crates.io/crates/redox_syscall/0.5.18) |
| `redox_users` | `0.5.2` | `MIT` | [crate](https://crates.io/crates/redox_users/0.5.2) |
| `regex` | `1.12.3` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/regex/1.12.3) |
| `regex-automata` | `0.4.14` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/regex-automata/0.4.14) |
| `regex-syntax` | `0.8.10` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/regex-syntax/0.8.10) |
| `rustc_version` | `0.4.1` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/rustc_version/0.4.1) |
| `rustix` | `0.38.44` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/rustix/0.38.44) |
| `rustix` | `1.1.4` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/rustix/1.1.4) |
| `rustversion` | `1.0.22` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/rustversion/1.0.22) |
| `ryu` | `1.0.23` | `Apache-2.0 OR BSL-1.0` | [crate](https://crates.io/crates/ryu/1.0.23) |
| `scopeguard` | `1.2.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/scopeguard/1.2.0) |
| `semver` | `1.0.28` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/semver/1.0.28) |
| `serde` | `1.0.228` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/serde/1.0.228) |
| `serde_core` | `1.0.228` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/serde_core/1.0.228) |
| `serde_derive` | `1.0.228` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/serde_derive/1.0.228) |
| `serde_json` | `1.0.150` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/serde_json/1.0.150) |
| `shell-words` | `1.1.1` | `MIT/Apache-2.0` | [crate](https://crates.io/crates/shell-words/1.1.1) |
| `signal-hook` | `0.3.18` | `Apache-2.0/MIT` | [crate](https://crates.io/crates/signal-hook/0.3.18) |
| `signal-hook-mio` | `0.2.5` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/signal-hook-mio/0.2.5) |
| `signal-hook-registry` | `1.4.8` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/signal-hook-registry/1.4.8) |
| `smallvec` | `1.15.1` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/smallvec/1.15.1) |
| `static_assertions` | `1.1.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/static_assertions/1.1.0) |
| `strsim` | `0.11.1` | `MIT` | [crate](https://crates.io/crates/strsim/0.11.1) |
| `strum` | `0.26.3` | `MIT` | [crate](https://crates.io/crates/strum/0.26.3) |
| `strum_macros` | `0.26.4` | `MIT` | [crate](https://crates.io/crates/strum_macros/0.26.4) |
| `syn` | `2.0.117` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/syn/2.0.117) |
| `tempfile` | `3.27.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/tempfile/3.27.0) |
| `thiserror` | `2.0.18` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/thiserror/2.0.18) |
| `thiserror-impl` | `2.0.18` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/thiserror-impl/2.0.18) |
| `toml_datetime` | `0.7.5+spec-1.1.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/toml_datetime/0.7.5+spec-1.1.0) |
| `toml_edit` | `0.23.10+spec-1.0.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/toml_edit/0.23.10+spec-1.0.0) |
| `toml_parser` | `1.1.2+spec-1.1.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/toml_parser/1.1.2+spec-1.1.0) |
| `toml_writer` | `1.1.1+spec-1.1.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/toml_writer/1.1.1+spec-1.1.0) |
| `unicode-ident` | `1.0.24` | `(MIT OR Apache-2.0) AND Unicode-3.0` | [crate](https://crates.io/crates/unicode-ident/1.0.24) |
| `unicode-segmentation` | `1.13.2` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/unicode-segmentation/1.13.2) |
| `unicode-truncate` | `1.1.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/unicode-truncate/1.1.0) |
| `unicode-width` | `0.1.14` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/unicode-width/0.1.14) |
| `unicode-width` | `0.2.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/unicode-width/0.2.0) |
| `unicode-xid` | `0.2.6` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/unicode-xid/0.2.6) |
| `wasi` | `0.11.1+wasi-snapshot-preview1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wasi/0.11.1+wasi-snapshot-preview1) |
| `wasip2` | `1.0.3+wasi-0.2.9` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wasip2/1.0.3+wasi-0.2.9) |
| `wasip3` | `0.4.0+wasi-0.3.0-rc-2026-01-06` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wasip3/0.4.0+wasi-0.3.0-rc-2026-01-06) |
| `wasm-encoder` | `0.244.0` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wasm-encoder/0.244.0) |
| `wasm-metadata` | `0.244.0` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wasm-metadata/0.244.0) |
| `wasmparser` | `0.244.0` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wasmparser/0.244.0) |
| `winapi` | `0.3.9` | `MIT/Apache-2.0` | [crate](https://crates.io/crates/winapi/0.3.9) |
| `winapi-i686-pc-windows-gnu` | `0.4.0` | `MIT/Apache-2.0` | [crate](https://crates.io/crates/winapi-i686-pc-windows-gnu/0.4.0) |
| `winapi-x86_64-pc-windows-gnu` | `0.4.0` | `MIT/Apache-2.0` | [crate](https://crates.io/crates/winapi-x86_64-pc-windows-gnu/0.4.0) |
| `windows-link` | `0.2.1` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/windows-link/0.2.1) |
| `windows-sys` | `0.59.0` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/windows-sys/0.59.0) |
| `windows-sys` | `0.61.2` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/windows-sys/0.61.2) |
| `windows-targets` | `0.52.6` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/windows-targets/0.52.6) |
| `windows_aarch64_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/windows_aarch64_gnullvm/0.52.6) |
| `windows_aarch64_msvc` | `0.52.6` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/windows_aarch64_msvc/0.52.6) |
| `windows_i686_gnu` | `0.52.6` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/windows_i686_gnu/0.52.6) |
| `windows_i686_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/windows_i686_gnullvm/0.52.6) |
| `windows_i686_msvc` | `0.52.6` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/windows_i686_msvc/0.52.6) |
| `windows_x86_64_gnu` | `0.52.6` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/windows_x86_64_gnu/0.52.6) |
| `windows_x86_64_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/windows_x86_64_gnullvm/0.52.6) |
| `windows_x86_64_msvc` | `0.52.6` | `MIT OR Apache-2.0` | [crate](https://crates.io/crates/windows_x86_64_msvc/0.52.6) |
| `winnow` | `0.7.15` | `MIT` | [crate](https://crates.io/crates/winnow/0.7.15) |
| `winnow` | `1.0.3` | `MIT` | [crate](https://crates.io/crates/winnow/1.0.3) |
| `wit-bindgen` | `0.51.0` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wit-bindgen/0.51.0) |
| `wit-bindgen` | `0.57.1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wit-bindgen/0.57.1) |
| `wit-bindgen-core` | `0.51.0` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wit-bindgen-core/0.51.0) |
| `wit-bindgen-rust` | `0.51.0` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wit-bindgen-rust/0.51.0) |
| `wit-bindgen-rust-macro` | `0.51.0` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wit-bindgen-rust-macro/0.51.0) |
| `wit-component` | `0.244.0` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wit-component/0.244.0) |
| `wit-parser` | `0.244.0` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crate](https://crates.io/crates/wit-parser/0.244.0) |
| `zeroize` | `1.8.2` | `Apache-2.0 OR MIT` | [crate](https://crates.io/crates/zeroize/1.8.2) |
| `zmij` | `1.0.21` | `MIT` | [crate](https://crates.io/crates/zmij/1.0.21) |
