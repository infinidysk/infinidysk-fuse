# Changelog

## [0.10.1](https://github.com/infinidysk/infinidysk-fuse/compare/v0.10.0...v0.10.1) (2026-11-13)


### Bug Fixes

* **deps:** bump mio from 0.8.11 to 1.1.1 ([7527b5a](https://github.com/infinidysk/infinidysk-fuse/commit/7527b5ab47bded552849d6c6cae813d59e0b6c87))
* **deps:** bump the cargo-minor-patch group across 1 directory with 9 updates ([8f33e63](https://github.com/infinidysk/infinidysk-fuse/commit/8f33e63b24819e90a7dac93c91eb2bd4f6d5e893))
* **deps:** bump the cargo-minor-patch group across 1 directory with 9 updates ([20c075a](https://github.com/infinidysk/infinidysk-fuse/commit/20c075a5ca057a3077e79455a2c602004f5d916f))
* **mount:** enable nix socket support on macOS without unprivileged ([2611176](https://github.com/infinidysk/infinidysk-fuse/commit/26111763db47e22e2b2624d33e73da82078b1b38))
* **mount:** macOS builds without the unprivileged feature; CI runs tests ([52bf89b](https://github.com/infinidysk/infinidysk-fuse/commit/52bf89bdcd0fffc7365e0624c68594c5b5175435))
* **raw:** bound COPY_FILE_RANGE length to the 32-bit reply count ([69dbe61](https://github.com/infinidysk/infinidysk-fuse/commit/69dbe619865ce8288477a496772ef1f702249e27)), closes [#25](https://github.com/infinidysk/infinidysk-fuse/issues/25)
* **raw:** CREATE replies keep sub-second cache lifetimes ([40c487d](https://github.com/infinidysk/infinidysk-fuse/commit/40c487d578ccbe9f15a64674eaf236587f061492))
* **raw:** encode CREATE reply TTL fractions in nanoseconds ([418ce7f](https://github.com/infinidysk/infinidysk-fuse/commit/418ce7ffc7c5c0c4c83b26571e6fa98d87465c86)), closes [#27](https://github.com/infinidysk/infinidysk-fuse/issues/27)
* **raw:** keep the session running when the kernel rejects a notification ([ed86ca1](https://github.com/infinidysk/infinidysk-fuse/commit/ed86ca1bcddd34151c1dbc9bf0ed83aea1fd5ec8))
* **raw:** keep the session running when the kernel rejects a notification ([cc71f97](https://github.com/infinidysk/infinidysk-fuse/commit/cc71f9750546ca030dd6085b79dd12919285a36e)), closes [#9](https://github.com/infinidysk/infinidysk-fuse/issues/9)
* **raw:** large copy_file_range results no longer wrap to a wrong count ([89cadf2](https://github.com/infinidysk/infinidysk-fuse/commit/89cadf2c23c13d300139198b15e89c59155e8d97))
* **raw:** reject kernel requests whose length does not match the read ([bf69cf6](https://github.com/infinidysk/infinidysk-fuse/commit/bf69cf617986435ee84d2982636203b36932aebf))
* **raw:** reject request frames whose length disagrees with the read ([5befffd](https://github.com/infinidysk/infinidysk-fuse/commit/5befffde1befd380f5d3277cdffc7c0abdaa8faa)), closes [#31](https://github.com/infinidysk/infinidysk-fuse/issues/31)
* **raw:** reject xattr replies that do not fit the request ([921cf3a](https://github.com/infinidysk/infinidysk-fuse/commit/921cf3a8729acbb02084d52b2f84d004eb7fbdcb)), closes [#9](https://github.com/infinidysk/infinidysk-fuse/issues/9)
* **raw:** reply EIO for errno values above ELAST on FreeBSD ([e32ca70](https://github.com/infinidysk/infinidysk-fuse/commit/e32ca70c3594ca93b3a3696673f978e4d5ac5c27)), closes [#9](https://github.com/infinidysk/infinidysk-fuse/issues/9)
* **raw:** reply EIO for invalid filesystem errno values ([e711bbc](https://github.com/infinidysk/infinidysk-fuse/commit/e711bbc0f1d45b7e40be6118fcea1ee9196180ae)), closes [#9](https://github.com/infinidysk/infinidysk-fuse/issues/9)
* **raw:** send EIO instead of invalid errno or xattr replies ([709e83d](https://github.com/infinidysk/infinidysk-fuse/commit/709e83d12c18246440837cd57c60c46ed3ed54e7))
* send filenames and data in kernel notifications ([35d62e0](https://github.com/infinidysk/infinidysk-fuse/commit/35d62e080f10217b77adb35d4389b09e5b6bf496))
* send filenames and data in kernel notifications ([e69cef9](https://github.com/infinidysk/infinidysk-fuse/commit/e69cef945229af6bd292980c3a3dd1081bf8c9c2))
* **session:** a failed session no longer leaves a dead mount behind ([2e7a41e](https://github.com/infinidysk/infinidysk-fuse/commit/2e7a41eb7127929e65fee8ff2e77986a36b21f58))
* **session:** answer a rejected reply with EIO on Linux ([e74694b](https://github.com/infinidysk/infinidysk-fuse/commit/e74694b7a5a2c3a8a324a71c38d2079623495f0a)), closes [#9](https://github.com/infinidysk/infinidysk-fuse/issues/9)
* **session:** catch a ready panic raised before its future exists ([62860d9](https://github.com/infinidysk/infinidysk-fuse/commit/62860d9bca154b78f3615b3f1c261f7bad58eb37))
* **session:** destroy waits for running request handlers ([3e1e971](https://github.com/infinidysk/infinidysk-fuse/commit/3e1e9714e4cf2000c04f00088a01d0bb73ce4337))
* **session:** end the session on persistent read errors instead of retrying forever ([55088b8](https://github.com/infinidysk/infinidysk-fuse/commit/55088b87ec8d275ab127f6aeb7a0038b42f5a0e7))
* **session:** keep blocked notifications from holding back replies ([34a674b](https://github.com/infinidysk/infinidysk-fuse/commit/34a674b782f5b59080840b45739ba3441cafd7cb)), closes [#8](https://github.com/infinidysk/infinidysk-fuse/issues/8)
* **session:** keep serving on Linux when the kernel rejects one reply ([07682e7](https://github.com/infinidysk/infinidysk-fuse/commit/07682e7c8cb5dfcfa725e2b3d90f1d718e7b5c95))
* **session:** retry only transient read errors ([155a40f](https://github.com/infinidysk/infinidysk-fuse/commit/155a40fb7f865ef6949cdd3b714d9c7efabfa02b)), closes [#31](https://github.com/infinidysk/infinidysk-fuse/issues/31)
* **session:** settle request handlers before destroy ([49ca1d2](https://github.com/infinidysk/infinidysk-fuse/commit/49ca1d20e05f68b1e546a4861a8f55fc3e7abe1a)), closes [#14](https://github.com/infinidysk/infinidysk-fuse/issues/14) [#15](https://github.com/infinidysk/infinidysk-fuse/issues/15)
* **session:** stop notifications from deadlocking replies on Linux ([603bf20](https://github.com/infinidysk/infinidysk-fuse/commit/603bf200a06a85507f1ea687c2b90adc87bacb66))
* **session:** unmount after a session error ([bf5e8f6](https://github.com/infinidysk/infinidysk-fuse/commit/bf5e8f6032a5aa38cf568f04814e110b28808a33)), closes [#15](https://github.com/infinidysk/infinidysk-fuse/issues/15)


### Refactors

* use IoSlice for vectored writes ([3e3d482](https://github.com/infinidysk/infinidysk-fuse/commit/3e3d482865771fe420ad239a2ac38965d42ab363))


### Build System

* name the problem when zero or two runtimes are enabled ([59e223a](https://github.com/infinidysk/infinidysk-fuse/commit/59e223a0c58cb0d19eb292030106e7e04a244f5e))
