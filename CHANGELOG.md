# Changelog

All notable changes to this project will be documented in this file.

See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

---
## [2.4.0](https://github.com/beyondessential/tamanu-meta-server/compare/v2.3.3..2.4.0) - 2024-09-19


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
