# Changelog

All notable changes to this project will be documented in this file.

See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

---
## [4.2.6](https://github.com/beyondessential/tamanu-meta-server/compare/v4.2.5..4.2.6) - 2025-08-19


- **Tweak:** Recording the hostname is more meaningful - ([4a0363c](https://github.com/beyondessential/tamanu-meta-server/commit/4a0363ca438bdb12f6f5113ee43b5e8c7cdac466))

---
## [4.2.5](https://github.com/beyondessential/tamanu-meta-server/compare/v4.2.4..v4.2.5) - 2025-08-19


- **Bugfix:** Exclude from default views the own-server - ([484bf62](https://github.com/beyondessential/tamanu-meta-server/commit/484bf620538dc478e13b6db1927ab8c9c285833c))

---
## [4.2.4](https://github.com/beyondessential/tamanu-meta-server/compare/v4.2.3..v4.2.4) - 2025-08-19


- **Feature:** Record own status - ([90a5f9a](https://github.com/beyondessential/tamanu-meta-server/commit/90a5f9acc48f2b013b781c78937e81009d69b20d))

---
## [4.2.3](https://github.com/beyondessential/tamanu-meta-server/compare/v4.2.2..v4.2.3) - 2025-08-18


- **Feature:** Add server timing header - ([26546a3](https://github.com/beyondessential/tamanu-meta-server/commit/26546a3cffc457c67794546ccf11755170add196))
- **Refactor:** Optimise the status fetch - ([ddf76a0](https://github.com/beyondessential/tamanu-meta-server/commit/ddf76a04318f37a0770dd98e8304458e7993c428))
- **Repo:** Add some token-saving rules for agent - ([039caca](https://github.com/beyondessential/tamanu-meta-server/commit/039caca93a7190c65eea8ac362e82ade75817f99))
- **Tweak:** Always return 404 for missing resources - ([db0aeef](https://github.com/beyondessential/tamanu-meta-server/commit/db0aeef4f51cdb124bba6bcf793b83c981d3e5eb))

---
## [4.2.2](https://github.com/beyondessential/tamanu-meta-server/compare/v4.2.1..v4.2.2) - 2025-08-16


- **Bugfix:** Return proper error codes for auth - ([c404691](https://github.com/beyondessential/tamanu-meta-server/commit/c404691d7bf322711d7676bdca80eee55139188f))
- **Documentation:** Document new auth errors - ([8da9192](https://github.com/beyondessential/tamanu-meta-server/commit/8da9192957be8a7096905d0c912774b15bc718a1))
- **Test:** Add more /servers tests - ([1f76d1b](https://github.com/beyondessential/tamanu-meta-server/commit/1f76d1b64b437328b6e84bca5b38993671397f56))

---
## [4.2.1](https://github.com/beyondessential/tamanu-meta-server/compare/v4.2.0..v4.2.1) - 2025-08-16


- **Bugfix:** Decode encoded tailscale name - ([b94602b](https://github.com/beyondessential/tamanu-meta-server/commit/b94602b6949179592bbc1224a4f656ac4c1c39e3))

---
## [4.2.0](https://github.com/beyondessential/tamanu-meta-server/compare/v4.1.6..v4.2.0) - 2025-08-16


- **Deps:** Upgrade compatibles - ([be66875](https://github.com/beyondessential/tamanu-meta-server/commit/be66875fb8dba84f3b7609d5cb058eba30b30ee4))
- **Deps:** Upgrade diesel to 2.2 - ([4a96666](https://github.com/beyondessential/tamanu-meta-server/commit/4a966664dd4b85582e1fe071481977ecbb0b0312))
- **Deps:** Update transitives - ([b21448f](https://github.com/beyondessential/tamanu-meta-server/commit/b21448f45ed5785802560a340e4b27e86e640c9c))
- **Deps:** Fix mobc version - ([255ceb9](https://github.com/beyondessential/tamanu-meta-server/commit/255ceb97e761b08317dda5cc51bf21481bc13b3c))
- **Deps:** Upgrade lockfile - ([aae959f](https://github.com/beyondessential/tamanu-meta-server/commit/aae959f99fcaf02a2dec8f71cec45b7a5b9bffc0))
- **Deps:** Bump actions/checkout from 4 to 5 (#51) - ([afa82d3](https://github.com/beyondessential/tamanu-meta-server/commit/afa82d310d25ee45b6342d850169a2ce3323ce15))
- **Feature:** Decouple devices and their keys - ([a1da2b2](https://github.com/beyondessential/tamanu-meta-server/commit/a1da2b2236711c05ede2d303ece2d128af129e03))
- **Feature:** Greet the user - ([48f1762](https://github.com/beyondessential/tamanu-meta-server/commit/48f1762e4ae54c3d124b6e1b1478397ce6bfa9ce))

---
## [4.1.6](https://github.com/beyondessential/tamanu-meta-server/compare/v4.1.5..v4.1.6) - 2025-08-16


- **Feature:** Display nodejs version - ([b26041f](https://github.com/beyondessential/tamanu-meta-server/commit/b26041f0a56ebe1ed2f4d231f03dab36c3063e93))
- **Test:** Fix clientip testing - ([5ed3ba1](https://github.com/beyondessential/tamanu-meta-server/commit/5ed3ba13eeee65c0a94006f4011db6049c3bbc05))
- **Tweak:** No longer store status errors - ([520a466](https://github.com/beyondessential/tamanu-meta-server/commit/520a46603941f049dd63a05c5f40cffc50d05155))
- **Tweak:** Remove unused latest_statuses view - ([520a466](https://github.com/beyondessential/tamanu-meta-server/commit/520a46603941f049dd63a05c5f40cffc50d05155))
- **Tweak:** Remove statuses.remote_ip - ([d88e4de](https://github.com/beyondessential/tamanu-meta-server/commit/d88e4de2a15937f00779b3164b316fa46926ba14))
- **Tweak:** Remove statuses.latency_ms - ([b0a622c](https://github.com/beyondessential/tamanu-meta-server/commit/b0a622c97a923e0f73241a3383dcd82417d60057))
- **Tweak:** Add btree on statuses.created_at for sorting performance - ([f2aee12](https://github.com/beyondessential/tamanu-meta-server/commit/f2aee12df04fc3ed39280f2b9c5f5cbc2e16bc79))

---
## [4.1.5](https://github.com/beyondessential/tamanu-meta-server/compare/v4.1.4..v4.1.5) - 2025-08-16


- **Feature:** More robust client ip parsing - ([b1b8d25](https://github.com/beyondessential/tamanu-meta-server/commit/b1b8d255b6ab7fb902fdc6cbf3062f49a94ad937))
- **Tweak:** Add client ip to logs - ([f0807bc](https://github.com/beyondessential/tamanu-meta-server/commit/f0807bc7a7c061bf66334f777821288d54bf5aa0))

---
## [4.1.4](https://github.com/beyondessential/tamanu-meta-server/compare/v4.1.3..v4.1.4) - 2025-08-16


- **Bugfix:** Revert pingtask to 10s - ([24669d5](https://github.com/beyondessential/tamanu-meta-server/commit/24669d5b42ffdb4bf79c98acf440a5dba3f8408d))

---
## [4.1.3](https://github.com/beyondessential/tamanu-meta-server/compare/v4.1.2..v4.1.3) - 2025-08-16


- **Bugfix:** Connect info layer - ([3132150](https://github.com/beyondessential/tamanu-meta-server/commit/313215022b8cafa79d37cebf97dfe2935744c037))
- **Bugfix:** Re-enable connectinfo ip fallback - ([70b4422](https://github.com/beyondessential/tamanu-meta-server/commit/70b44221968c145f699885fbfa5f760914ac5761))

---
## [4.1.2](https://github.com/beyondessential/tamanu-meta-server/compare/v4.1.1..v4.1.2) - 2025-08-16


- **Bugfix:** Re-enable TLS for ping client - ([c7c4096](https://github.com/beyondessential/tamanu-meta-server/commit/c7c4096edad1c97d7206e396aea8e17077ab74d2))
- **Tweak:** Stop collecting ping errors - ([5befb8e](https://github.com/beyondessential/tamanu-meta-server/commit/5befb8ef017b5ac4e20148f2c83712ee5c1c6673))

---
## [4.1.0](https://github.com/beyondessential/tamanu-meta-server/compare/v4.0.6..v4.1.0) - 2025-08-16


- **Bugfix:** Fix public port - ([a8117ac](https://github.com/beyondessential/tamanu-meta-server/commit/a8117acb508747cfbf27cdf1595101f4c164b78c))
- **Bugfix:** Fix logging - ([33af436](https://github.com/beyondessential/tamanu-meta-server/commit/33af4367ff8b11f78f4a2b49f7bd1ad19a2f8f5a))
- **Bugfix:** Fix error type uris to ourselves - ([43d38cf](https://github.com/beyondessential/tamanu-meta-server/commit/43d38cf73eb1de8ad1a79343a3a8f419d356406c))
- **Bugfix:** Fix docker build - ([1962d6b](https://github.com/beyondessential/tamanu-meta-server/commit/1962d6b745bf69baff0985e1809157c7dfbdabac))
- **Bugfix:** Fix test - ([e8a69d9](https://github.com/beyondessential/tamanu-meta-server/commit/e8a69d9dac6727890c0f3ea1f7a672413d7423c2))
- **Bugfix:** Fix migrator error - ([78356f7](https://github.com/beyondessential/tamanu-meta-server/commit/78356f742440b3bee98c2c6659f4822021d3b478))
- **Documentation:** Document errors - ([0c8e1d6](https://github.com/beyondessential/tamanu-meta-server/commit/0c8e1d6a1d5c5cb730ad5714e85a6ff90582be3c))
- **Refactor:** Add extra() convenience accessor - ([25e1189](https://github.com/beyondessential/tamanu-meta-server/commit/25e11898faa41d54bd618d975cf03bcf59e129ba))
- **Test:** Add test harness and device fkey behaviour check - ([0c8af78](https://github.com/beyondessential/tamanu-meta-server/commit/0c8af784123bcb474cd93c21256f764472c890fc))
- **Test:** Test /servers exclusion - ([cc2d10a](https://github.com/beyondessential/tamanu-meta-server/commit/cc2d10a3021da3cc8517d5fd7cc6fc44b3704428))
- **Tweak:** Track device that originated a status - ([c8135c6](https://github.com/beyondessential/tamanu-meta-server/commit/c8135c6aa1f73b22a7143faa717e289236ffcef9))
- **Tweak:** Obtain device connection for status - ([afce121](https://github.com/beyondessential/tamanu-meta-server/commit/afce1216f6c86c13d45d059f8a208b132d6f7874))
- **Tweak:** Obtain the connection info for this status - ([a0ad670](https://github.com/beyondessential/tamanu-meta-server/commit/a0ad670915ac200560beb0500e01bbef142125b1))

---
## [4.0.6](https://github.com/beyondessential/tamanu-meta-server/compare/v4.0.5..v4.0.6) - 2025-08-06


- **Tweak:** Allow leading v in versions in api - ([7089e3f](https://github.com/beyondessential/tamanu-meta-server/commit/7089e3f241bf3b7b62c10a515191580a32e06943))

---
## [4.0.5](https://github.com/beyondessential/tamanu-meta-server/compare/v4.0.4..v4.0.5) - 2025-07-31


- **Tweak:** Support version ranges on all GET /versions routes - ([63a5c86](https://github.com/beyondessential/tamanu-meta-server/commit/63a5c868881e14b25a41b2a1a2621b8ddaff34f8))

---
## [4.0.4](https://github.com/beyondessential/tamanu-meta-server/compare/v4.0.3..v4.0.4) - 2025-07-23


- **Bugfix:** Fix something using a float - ([3db60e3](https://github.com/beyondessential/tamanu-meta-server/commit/3db60e361ef49e5aaad79f5e380eab5df05e35b7))

---
## [4.0.3](https://github.com/beyondessential/tamanu-meta-server/compare/v4.0.2..v4.0.3) - 2025-07-23


- **Feature:** Add env vars to home page - ([6bc23c9](https://github.com/beyondessential/tamanu-meta-server/commit/6bc23c9becc9d980a3df3252e61590775b530e1f))

---
## [4.0.2](https://github.com/beyondessential/tamanu-meta-server/compare/v4.0.1..v4.0.2) - 2025-07-23


- **Feature:** Add health endpoints - ([7341772](https://github.com/beyondessential/tamanu-meta-server/commit/7341772ad15a96e474dbf9efce86274c67ecf9ac))

---
## [4.0.1](https://github.com/beyondessential/tamanu-meta-server/compare/v4.0.0..v4.0.1) - 2025-07-23


- **Feature:** Add prefix to private server - ([abe9f27](https://github.com/beyondessential/tamanu-meta-server/commit/abe9f27ad7e3f5909680266a1fa702cd78dbabfd))

---
## [4.0.0](https://github.com/beyondessential/tamanu-meta-server/compare/v3.5.4..v4.0.0) - 2025-07-23


- **Feature:** Split public (internet) and private (tailscale) servers - ([7c4d90e](https://github.com/beyondessential/tamanu-meta-server/commit/7c4d90e740329c8ac8238c9afdf636b80c2eecba))

---
## [3.5.4](https://github.com/beyondessential/tamanu-meta-server/compare/v3.5.3..v3.5.4) - 2025-07-23


- **Feature:** Differentiate between facilities and other servers in status - ([622fd4d](https://github.com/beyondessential/tamanu-meta-server/commit/622fd4dfdbd57d41192306f981a3f2d802301383))

---
## [3.5.3](https://github.com/beyondessential/tamanu-meta-server/compare/v3.5.2..v3.5.3) - 2025-07-23


- **Bugfix:** Fix is_up in status view - ([0ccc62a](https://github.com/beyondessential/tamanu-meta-server/commit/0ccc62a0fe6580155f8dae42fa4bb45c58e281ed))

---
## [3.5.2](https://github.com/beyondessential/tamanu-meta-server/compare/v3.5.1..v3.5.2) - 2025-05-21



### Password

- **Tweak:** Do not store generated or verified passwords - ([5f98ff2](https://github.com/beyondessential/tamanu-meta-server/commit/5f98ff21f5b1dce912a32c3fcdc154d9322b4f3b))
- **Tweak:** Vanity prefix for Other category - ([23ce06a](https://github.com/beyondessential/tamanu-meta-server/commit/23ce06a5bfd24ecf2a8d81b958c7982141ca495d))

---
## [3.5.1](https://github.com/beyondessential/tamanu-meta-server/compare/v3.5.0..v3.5.1) - 2025-05-21



### Password

- **Feature:** Add verifier - ([c46a8a9](https://github.com/beyondessential/tamanu-meta-server/commit/c46a8a9431722bddaa0238b6d3b8352ba65e7e1c))
- **Tweak:** Add checksum - ([f652c6c](https://github.com/beyondessential/tamanu-meta-server/commit/f652c6ceb8fe734756ca73f1d40234d9388ddc39))

---
## [3.5.0](https://github.com/beyondessential/tamanu-meta-server/compare/v3.4.0..v3.5.0) - 2025-05-21


- **Feature:** Add password generator - ([6ed28f4](https://github.com/beyondessential/tamanu-meta-server/commit/6ed28f498f095753d58fdc19165c0a6a78a6539a))

---
## [3.4.0](https://github.com/beyondessential/tamanu-meta-server/compare/v3.3.0..v3.4.0) - 2025-05-08


- **Tweak:** Add QR code to installers page (#41) - ([979c00f](https://github.com/beyondessential/tamanu-meta-server/commit/979c00fc2ca0f71b91efbd879f64332358301779))
- **Tweak:** Use svg logo on installers page (#42) - ([74f302d](https://github.com/beyondessential/tamanu-meta-server/commit/74f302d37debd06be0f4cdd372af3998cfa94730))

---
## [3.3.0](https://github.com/beyondessential/tamanu-meta-server/compare/v3.2.7..v3.3.0) - 2025-04-28


- **Feature:** Add timesync server (#40) - ([c283a22](https://github.com/beyondessential/tamanu-meta-server/commit/c283a222087cd135bd33fbe85312e08ce665f957))

---
## [3.2.7](https://github.com/beyondessential/tamanu-meta-server/compare/v3.2.6..v3.2.7) - 2025-04-03


- **Documentation:** More db and api instructions - ([f25658e](https://github.com/beyondessential/tamanu-meta-server/commit/f25658ed1082cad345650d663fcfcfe592e0f5bc))
- **Documentation:** Need git-cliff to release - ([d02505e](https://github.com/beyondessential/tamanu-meta-server/commit/d02505e2cab9d76a67546d138e0ed61dbebd878c))
- **Documentation:** Add trust info - ([68abfa0](https://github.com/beyondessential/tamanu-meta-server/commit/68abfa096b070d2c878e5525ac818c41cd1bc955))

### Identity

- **Tweak:** Write public key - ([a46e435](https://github.com/beyondessential/tamanu-meta-server/commit/a46e435d43ddce79db7d85f48eb7e1dd0cb912ef))

### Versions

- **Bugfix:** Allow large changelogs (up to 1MB) - ([f61eca8](https://github.com/beyondessential/tamanu-meta-server/commit/f61eca8f4ad251ee5cda710417d1939a308c6a1d))

---
## [3.2.6](https://github.com/beyondessential/tamanu-meta-server/compare/v3.2.5..v3.2.6) - 2025-03-28


- **Bugfix:** Migration order - ([7570bbb](https://github.com/beyondessential/tamanu-meta-server/commit/7570bbb22c2de18875996b416d74468bb6a26c70))
- **Bugfix:** Hide facility servers in /servers (for mobile) - ([06eb08c](https://github.com/beyondessential/tamanu-meta-server/commit/06eb08c40f761c2fee3cbec9338a3469cf261bf5))

---
## [3.2.5](https://github.com/beyondessential/tamanu-meta-server/compare/v3.2.4..v3.2.5) - 2025-03-28


- **Refactor:** Add error handling on /status page - ([fef43fc](https://github.com/beyondessential/tamanu-meta-server/commit/fef43fc544a9219aeb6818949dc682688b1f6018))

---
## [3.2.4](https://github.com/beyondessential/tamanu-meta-server/compare/v3.2.3..v3.2.4) - 2025-03-28


- **Tweak:** Only show public fields on /servers route - ([e85402e](https://github.com/beyondessential/tamanu-meta-server/commit/e85402e07c39f8a90bf0141e779905e8e9a73aec))

---
## [3.2.3](https://github.com/beyondessential/tamanu-meta-server/compare/v3.2.2..v3.2.3) - 2025-03-28


- **Tweak:** Put changelog after artifacts - ([7ae067a](https://github.com/beyondessential/tamanu-meta-server/commit/7ae067add80653436d3c18b76f3cf75fc1f53393))

---
## [3.2.2](https://github.com/beyondessential/tamanu-meta-server/compare/v3.2.1..v3.2.2) - 2025-03-28


- **Tweak:** Don’t poll servers that are pushing statuses - ([1d26000](https://github.com/beyondessential/tamanu-meta-server/commit/1d26000e3d0e2bab8deaca9df1c01fbcff571cf9))

---
## [3.2.1](https://github.com/beyondessential/tamanu-meta-server/compare/v3.2.0..v3.2.1) - 2025-03-28


- **Feature:** Pull the server device_id from the submitting device - ([bd2ecb6](https://github.com/beyondessential/tamanu-meta-server/commit/bd2ecb68dccd1c5e59d005dbcdee6a9bd00e3c74))
- **Tweak:** Shift statuses.server_type to servers.kind - ([f7a6e15](https://github.com/beyondessential/tamanu-meta-server/commit/f7a6e155cd5ad0049f6d1665cd7b3076b728793f))
- **Tweak:** No longer do an upsert on POST /servers - ([c4d6f20](https://github.com/beyondessential/tamanu-meta-server/commit/c4d6f20e9542ab2db47be60121b423949ebcf408))

---
## [3.2.0](https://github.com/beyondessential/tamanu-meta-server/compare/v3.1.5..v3.2.0) - 2025-03-27


- **Feature:** Update server post logic (#38) - ([af48475](https://github.com/beyondessential/tamanu-meta-server/commit/af48475ae59425126e8fb63ed68a91ba9d5eae01))

---
## [3.1.5](https://github.com/beyondessential/tamanu-meta-server/compare/v3.1.4..v3.1.5) - 2025-03-25


- **Tweak:** Support ssl-client-cert alternative header - ([aa0d989](https://github.com/beyondessential/tamanu-meta-server/commit/aa0d989d8fcb2a1d3a682bdddbb37a153d933a3f))

---
## [3.1.4](https://github.com/beyondessential/tamanu-meta-server/compare/v3.1.3..v3.1.4) - 2025-03-25


- **Tweak:** Workaround for a mistake in Tamanu - ([b1a1e07](https://github.com/beyondessential/tamanu-meta-server/commit/b1a1e07c8c6eafd4ef05f94437da505d6b160f3a))

---
## [3.1.3](https://github.com/beyondessential/tamanu-meta-server/compare/v3.1.2..v3.1.3) - 2025-03-25


- **Tweak:** Enable tables and github-style markdown options - ([6a47070](https://github.com/beyondessential/tamanu-meta-server/commit/6a470706e5e21b51c16549686f71a627bd6db4aa))

---
## [3.1.2](https://github.com/beyondessential/tamanu-meta-server/compare/v3.1.1..v3.1.2) - 2025-03-25


- **Tweak:** Match client header case-insensitively - ([395f35f](https://github.com/beyondessential/tamanu-meta-server/commit/395f35fbfacfbd3ee4b4ea792de611b747008ba6))

---
## [3.1.1](https://github.com/beyondessential/tamanu-meta-server/compare/v3.1.0..v3.1.1) - 2025-03-25


- **Bugfix:** Fix header names (#36) - ([7a42367](https://github.com/beyondessential/tamanu-meta-server/commit/7a42367a70defb3591b9b90c4c3a58714b9314c1))
- **Tweak:** Strip device_id from server list - ([fd74160](https://github.com/beyondessential/tamanu-meta-server/commit/fd741606f595e1066ad03d3e7cc4a7d3e5f47770))
- **Tweak:** Strip device_id from server list entirely - ([bbd2393](https://github.com/beyondessential/tamanu-meta-server/commit/bbd23939fb6084090f3b813acc291409c35f617d))

---
## [3.1.0](https://github.com/beyondessential/tamanu-meta-server/compare/v3.0.4..v3.1.0) - 2025-03-25


- **Feature:** Parse release notes markdown (#35) - ([e69c440](https://github.com/beyondessential/tamanu-meta-server/commit/e69c440a85e96a4f6ec2a828565b086e40a4d52e))

### Statuses

- **Feature:** KAM-65: Add post endpoint for creating status entry (#33) - ([cc3ebc4](https://github.com/beyondessential/tamanu-meta-server/commit/cc3ebc450af879ac43bc47cdac86ea72cd12ba46))

---
## [3.0.4](https://github.com/beyondessential/tamanu-meta-server/compare/v3.0.3..v3.0.4) - 2025-03-21


- **Bugfix:** Range matching - ([30c63dd](https://github.com/beyondessential/tamanu-meta-server/commit/30c63ddb08a1e51dea983bc38ce1bcbe68f54aed))

---
## [3.0.3](https://github.com/beyondessential/tamanu-meta-server/compare/v3.0.2..v3.0.3) - 2025-03-21


- **Bugfix:** Back link on artifact page - ([9142bc8](https://github.com/beyondessential/tamanu-meta-server/commit/9142bc8891f5b94b3f597241dbb94aed2eb691f7))

---
## [3.0.2](https://github.com/beyondessential/tamanu-meta-server/compare/v3.0.1..v3.0.2) - 2025-03-21


- **Bugfix:** Only show download button if https - ([df3d2ec](https://github.com/beyondessential/tamanu-meta-server/commit/df3d2ec9e74140f95301f1e7bad3c076b10d042f))

---
## [3.0.1](https://github.com/beyondessential/tamanu-meta-server/compare/v3.0.0..v3.0.1) - 2025-03-21


- **Bugfix:** Artifact button - ([fd148a7](https://github.com/beyondessential/tamanu-meta-server/commit/fd148a72a52ddaea2b9a4fdf11e4b2ca4c934acc))

---
## [3.0.0](https://github.com/beyondessential/tamanu-meta-server/compare/v2.6.1..v3.0.0) - 2025-03-21


- **Tweak:** Add versions list link on index - ([2e37c9e](https://github.com/beyondessential/tamanu-meta-server/commit/2e37c9e3c75e101b95efa85cac7620e65bf9dc1a))
- **Tweak:** Make versions the default view - ([afa12b8](https://github.com/beyondessential/tamanu-meta-server/commit/afa12b84c61d6e55004986750ec9f26de601fe78))

---
## [2.6.1](https://github.com/beyondessential/tamanu-meta-server/compare/v2.6.0..v2.6.1) - 2025-03-21



### Artifacts

- **Tweak:** Fix sort order - ([50a263b](https://github.com/beyondessential/tamanu-meta-server/commit/50a263b596f3fee14ad652a7b2ce05c60e4542f7))

---
## [2.6.0](https://github.com/beyondessential/tamanu-meta-server/compare/v2.5.3..v2.6.0) - 2025-03-21



### Artifacts

- **Feature:** Add mobile download page - ([7b135ab](https://github.com/beyondessential/tamanu-meta-server/commit/7b135ab2e12a3405fc899478ea2f00c20a301516))

---
## [2.5.3](https://github.com/beyondessential/tamanu-meta-server/compare/v2.5.2..v2.5.3) - 2025-03-21



### Artifacts

- **Refactor:** Move database interaction to db/ - ([f22a00c](https://github.com/beyondessential/tamanu-meta-server/commit/f22a00c0a17ebc380e72cce93de26edc3e2642ec))
- **Tweak:** Invert platform and type - ([1ca9aa3](https://github.com/beyondessential/tamanu-meta-server/commit/1ca9aa34555673118feb7215064d7ec77c4b1dfc))
- **Tweak:** Simplify route - ([3d2e00e](https://github.com/beyondessential/tamanu-meta-server/commit/3d2e00e0f901149e01438068eff827b320fa9b8c))

---
## [2.5.2](https://github.com/beyondessential/tamanu-meta-server/compare/v2.5.1..v2.5.2) - 2025-03-21


- **Bugfix:** Artifact endpoint - ([f969aa4](https://github.com/beyondessential/tamanu-meta-server/commit/f969aa464426de6d09c66068adb6de4b2fcd6b34))

---
## [2.5.1](https://github.com/beyondessential/tamanu-meta-server/compare/v2.5.0..v2.5.1) - 2025-03-21


- **Bugfix:** POST /version defaulting to published=false - ([421a49c](https://github.com/beyondessential/tamanu-meta-server/commit/421a49c9f4567536b0aecc5bcf3f77929b5294cc))
- **Bugfix:** Fix build - ([1db6304](https://github.com/beyondessential/tamanu-meta-server/commit/1db63049c863c4b82ace6237c2d6310331e04fe0))

---
## [2.5.0](https://github.com/beyondessential/tamanu-meta-server/compare/v2.4.2..v2.5.0) - 2025-03-21


- **Feature:** Add versions table (#29) - ([5fcd4ab](https://github.com/beyondessential/tamanu-meta-server/commit/5fcd4ab601355c4dc8155e12dba6e055a369654b))

### Auth

- **Feature:** Add devices and finer-grained authentication (#31) - ([8761dbf](https://github.com/beyondessential/tamanu-meta-server/commit/8761dbfb5057848f8eb6765d5657ac5a0b52dfba))

---
## [2.4.2](https://github.com/beyondessential/tamanu-meta-server/compare/v2.4.1..v2.4.2) - 2025-02-26


- **Deps:** Update lockfile - ([e646644](https://github.com/beyondessential/tamanu-meta-server/commit/e64664465cae7e0e9988e8f6e7c3cd25e43b4864))
- **Documentation:** /versions - ([70af0b5](https://github.com/beyondessential/tamanu-meta-server/commit/70af0b50efb09144e61e64fbe90c543fda90b84f))

---
## [2.4.1](https://github.com/beyondessential/tamanu-meta-server/compare/v2.4.0..v2.4.1) - 2024-09-19


- **Bugfix:** Unique versions and show branches - ([e2eea01](https://github.com/beyondessential/tamanu-meta-server/commit/e2eea01d9ac924ae739ebdc93f28eaa540e06b2e))

---
## [2.4.0](https://github.com/beyondessential/tamanu-meta-server/compare/v2.3.3..v2.4.0) - 2024-09-19


- **Feature:** Display min and max versions in active use (#6) - ([998f39b](https://github.com/beyondessential/tamanu-meta-server/commit/998f39b37515426b1710dac00fcc76d228a0de89))
- **Repo:** Add release instructions - ([635c290](https://github.com/beyondessential/tamanu-meta-server/commit/635c2904b70a5a7b95b0cb79fe61b4242fa18798))

---
## [2.3.3](https://github.com/beyondessential/tamanu-meta-server/compare/v2.3.2..v2.3.3) - 2024-09-13


- **Bugfix:** Serialize UrlField without trailing slash - ([2c10f01](https://github.com/beyondessential/tamanu-meta-server/commit/2c10f018a2e8b0f5a7393bfcc6cfca78b5ea94d9))

---
## [2.3.2](https://github.com/beyondessential/tamanu-meta-server/compare/v2.3.1..v2.3.2) - 2024-09-05


- **Tweak:** Use sans-serif font to aid clarity - ([af575d7](https://github.com/beyondessential/tamanu-meta-server/commit/af575d7391319b2dd603abb45519a3b8780c9b11))

---
## [2.3.1](https://github.com/beyondessential/tamanu-meta-server/compare/v2.3.0..v2.3.1) - 2024-09-05


- **Documentation:** Expose "hidden" copy feature - ([598054e](https://github.com/beyondessential/tamanu-meta-server/commit/598054ee9e66de7afc6c568a78c835432a80f533))
- **Documentation:** Explain how to do migrations in dev - ([91b6152](https://github.com/beyondessential/tamanu-meta-server/commit/91b6152fef3cc7c35829579c43daa6e6d8663e9c))
- **Feature:** Use ordered_servers view transparently in place of servers table - ([6544306](https://github.com/beyondessential/tamanu-meta-server/commit/6544306a47cc1629ef5c579525fb209adc2008c4))

---
## [2.3.0](https://github.com/beyondessential/tamanu-meta-server/compare/v2.2.0..v2.3.0) - 2024-09-04


- **Documentation:** Fix repo clone url - ([2a999c7](https://github.com/beyondessential/tamanu-meta-server/commit/2a999c72e707b6342cc61bcbf334774bb099fcc6))
- **Feature:** Split pingtask - ([b322ab7](https://github.com/beyondessential/tamanu-meta-server/commit/b322ab71d0e0cdf46240d4845f376f1dfff5131f))

---
## [2.2.0](https://github.com/beyondessential/tamanu-meta-server/compare/v2.1.3..v2.2.0) - 2024-09-04


- **Feature:** Add test server rank - ([5320caf](https://github.com/beyondessential/tamanu-meta-server/commit/5320caf284d4e4c271093fe8f36ef0537413c58e))

---
## [2.1.3](https://github.com/beyondessential/tamanu-meta-server/compare/v2.1.2..v2.1.3) - 2024-09-04


- **Bugfix:** Whoops didn't check if things compiled before shipping - ([e1f60d4](https://github.com/beyondessential/tamanu-meta-server/commit/e1f60d42c37e7ce2326964cbd6d068c05a729a3b))
- **Deps:** Okay libpq is not friendly to windows and mac - ([0a242a7](https://github.com/beyondessential/tamanu-meta-server/commit/0a242a7fd424cd801c316d5a79582a45c59057ec))

---
## [2.1.2](https://github.com/beyondessential/tamanu-meta-server/compare/v2.1.1..v2.1.2) - 2024-09-04


- **Deps:** Use libpq in development - ([5ec597a](https://github.com/beyondessential/tamanu-meta-server/commit/5ec597aeb4e13d53ca068c03e4674072d70791d8))
- **Feature:** Embed tls roots in container - ([be4b84f](https://github.com/beyondessential/tamanu-meta-server/commit/be4b84f4faad6b80267e0402adfb1eaf6595d6a2))

---
## [2.1.1](https://github.com/beyondessential/tamanu-meta-server/compare/v2.1.0..v2.1.1) - 2024-09-04


- **Tweak:** Prettier source error - ([b57c491](https://github.com/beyondessential/tamanu-meta-server/commit/b57c491f02369e529cf53b848c13acd82f4f0a7a))

---
## [2.1.0](https://github.com/beyondessential/tamanu-meta-server/compare/v2.0.3..v2.1.0) - 2024-09-04


- **Documentation:** Describe the API - ([4cdab85](https://github.com/beyondessential/tamanu-meta-server/commit/4cdab85aea3e8e9450533a97e1e6c661fad3d1de))
- **Feature:** Rename type to rank on the api - ([ec0d979](https://github.com/beyondessential/tamanu-meta-server/commit/ec0d979ddbe597b2af551d18f22a821ad2637b20))
- **Repo:** Fix readme for new container version scheme - ([d44c15c](https://github.com/beyondessential/tamanu-meta-server/commit/d44c15cf7d1a1707913a9c4aca0a9d5f4a5586b7))
- **Tweak:** Use debug representation of http error for more clarity - ([ba3e6cb](https://github.com/beyondessential/tamanu-meta-server/commit/ba3e6cbc56e9b3855a0c17e31b79f94e6931487e))
- **Tweak:** Ping /api/public/ping for status, instead of the now-private bare /api - ([aaa34cb](https://github.com/beyondessential/tamanu-meta-server/commit/aaa34cb5cdde06618eed1e46c2f351c13cb8945e))

---
## [2.0.2](https://github.com/beyondessential/tamanu-meta-server/compare/v2.0.1..v2.0.2) - 2024-09-04



### Migrate

- **Bugfix:** Prepare migration table on every use - ([8cdf74b](https://github.com/beyondessential/tamanu-meta-server/commit/8cdf74bb9b4b5e5e4e2826af376a0f7585e991a6))

---
## [2.0.1](https://github.com/beyondessential/tamanu-meta-server/compare/v2.0.0..v2.0.1) - 2024-09-04


- **Repo:** Add changelog - ([b512cde](https://github.com/beyondessential/tamanu-meta-server/commit/b512cdea227ed6395c701341e258f3dc52882876))

### Migrate

- **Bugfix:** Migration hack did not allow running from scratch - ([ba5227d](https://github.com/beyondessential/tamanu-meta-server/commit/ba5227de167844ce3471a8d3230f79120a28ed13))

---
## [2.0.0] - 2024-09-03


- **Bugfix:** Handle graceful shutdowns - ([f00ce26](https://github.com/beyondessential/tamanu-meta-server/commit/f00ce26c500eddc4d1e408f00c54287e5136cf63))
- **Bugfix:** Set server as default so it's easier to run - ([432ac85](https://github.com/beyondessential/tamanu-meta-server/commit/432ac851f1c5b73212fbca07f1cc9b9bcc87fb83))
- **Deps:** Cargo update - ([013ab4a](https://github.com/beyondessential/tamanu-meta-server/commit/013ab4afbcb70c0a948d2d6e943063ce21784a53))
- **Deps:** Use latest rocket and diesel - ([019cd4c](https://github.com/beyondessential/tamanu-meta-server/commit/019cd4c6c5633c896176db4daae897e75ccc1f9c))
- **Deps:** Hack up migrate to ditch libpq dependency - ([cf6690b](https://github.com/beyondessential/tamanu-meta-server/commit/cf6690b1a76c49f838f72308d7a36e02cec8026a))
- **Documentation:** Mutual tls settings - ([6c1630a](https://github.com/beyondessential/tamanu-meta-server/commit/6c1630a2eeec4156346d94ddad17e8a4943bf35d))
- **Documentation:** Internally document the public api - ([2b8e205](https://github.com/beyondessential/tamanu-meta-server/commit/2b8e2053a2ab0cbf6bfd2c7c02c8d74d38f22c88))
- **Feature:** Add database backing servers - ([30c9333](https://github.com/beyondessential/tamanu-meta-server/commit/30c933378aead1cd0f087467ae9fc644c7e0f8e2))
- **Feature:** Insert statuses into the database - ([85ce0de](https://github.com/beyondessential/tamanu-meta-server/commit/85ce0de6f5c36807266e3249ca894cb4f72631d5))
- **Feature:** Regularly ping all servers - ([27ad3ea](https://github.com/beyondessential/tamanu-meta-server/commit/27ad3eae74de5252eccca0919536dcfcc2ff2490))
- **Feature:** Render statuses from database - ([dbdb3b3](https://github.com/beyondessential/tamanu-meta-server/commit/dbdb3b341c202208ee4032489fbddfc2355e372c))
- **Feature:** Add timeout to ping - ([4f6458f](https://github.com/beyondessential/tamanu-meta-server/commit/4f6458f67d6dfbb07e78072d68bda514932c3b53))
- **Feature:** Add manual reload route - ([5da00b8](https://github.com/beyondessential/tamanu-meta-server/commit/5da00b8a4f097d21a68e094ab57a08f6d00cc44d))
- **Feature:** Add mtls auth to mutating api routes - ([b2094b6](https://github.com/beyondessential/tamanu-meta-server/commit/b2094b685c1da14ba8657286c2e5b3e8765ae78e))
- **Feature:** Add hidden ability to copy server_id - ([147a358](https://github.com/beyondessential/tamanu-meta-server/commit/147a358e3195ce3c6402e837ad08e0a35bc52235))
- **Feature:** Add indices - ([f8c07c1](https://github.com/beyondessential/tamanu-meta-server/commit/f8c07c1a5b319b7b899227284172b09010ee15d5))
- **Feature:** Migrate tool with embedded migrations - ([b605a1b](https://github.com/beyondessential/tamanu-meta-server/commit/b605a1b1ed0d3153d7d4aaba62c13e11a3b2d6b5))
- **Feature:** Fully cross build container for gnu libc - ([41c3217](https://github.com/beyondessential/tamanu-meta-server/commit/41c32177433899dc052535cae1d20f55fb449445))
- **Feature:** Add clone to rank - ([b1145e2](https://github.com/beyondessential/tamanu-meta-server/commit/b1145e2fa6d022de48a3c47ff8b435013e045f5f))
- **Refactor:** Remove unused code - ([411ae36](https://github.com/beyondessential/tamanu-meta-server/commit/411ae36f6b5a332ea723ada18a1e2da27349bcd3))
- **Refactor:** Split into modules - ([158c159](https://github.com/beyondessential/tamanu-meta-server/commit/158c15967e04f56aa23f715a0d152e45b8f6ae3a))
- **Refactor:** Split into smaller modules - ([346d1d2](https://github.com/beyondessential/tamanu-meta-server/commit/346d1d2d930d00c75a3009463c9bd751be12c5b8))
- **Refactor:** Split statuses into smaller modules - ([17de00f](https://github.com/beyondessential/tamanu-meta-server/commit/17de00f47e00fc5614267a9d2a83660b4ffcbe22))
- **Repo:** Add meta files - ([0e4fe65](https://github.com/beyondessential/tamanu-meta-server/commit/0e4fe654bf56f45423eb27f48d3598421b70b51a))
- **Repo:** Add git-cliff - ([9c09957](https://github.com/beyondessential/tamanu-meta-server/commit/9c09957471730d36345923092473c5f103d4c25c))
- **Repo:** Don't publish - ([6d28364](https://github.com/beyondessential/tamanu-meta-server/commit/6d283641d6dc5f2aad8263863790df257f2b25ed))
- **Revert:** "deps: use latest rocket and diesel" - ([a535aa1](https://github.com/beyondessential/tamanu-meta-server/commit/a535aa10530ac09d7891e3785c263e457dcc37b0))

### Migrate

- **Tweak:** [STEPS] -> [N] - ([59f891d](https://github.com/beyondessential/tamanu-meta-server/commit/59f891d171ba4df7f347f6a81a7dbc14532a61bf))

<!-- generated by git-cliff -->
