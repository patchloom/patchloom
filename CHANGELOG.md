# Changelog

All notable changes to Patchloom are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

Curated release notes for the next version live in `RELEASE_NOTES.md` when
present (applied to the GitHub Release body by the host job). Versioned
sections below are managed by release-please.


## [0.12.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.11.0...patchloom-v0.12.0) (2026-07-11)


### Features

* **api:** LLM agent embedder surface (require_change, AST mutators, shell tokens, batch rename) ([#1497](https://github.com/patchloom/patchloom/issues/1497)) ([afd19b2](https://github.com/patchloom/patchloom/commit/afd19b215b2122d721fbff65f323a265ae8a05cf)), closes [#1492](https://github.com/patchloom/patchloom/issues/1492) [#1493](https://github.com/patchloom/patchloom/issues/1493) [#1494](https://github.com/patchloom/patchloom/issues/1494) [#1495](https://github.com/patchloom/patchloom/issues/1495)
* **api:** crate-root apply_content_edits_to_file re-export ([#1512](https://github.com/patchloom/patchloom/issues/1512)) ([1604d2e](https://github.com/patchloom/patchloom/commit/1604d2e3328bfa84d07aea76a35cedd634c163c9))
* **cli:** identity JSON flag and GNU --name=value option peeling ([#1543](https://github.com/patchloom/patchloom/issues/1543)) ([8615bb9](https://github.com/patchloom/patchloom/commit/8615bb994baa3f6b9f8eb197bc33b9a5a7d15070))
* **cli:** replace --command-position and --require-change ([#1523](https://github.com/patchloom/patchloom/issues/1523)) ([0c53415](https://github.com/patchloom/patchloom/commit/0c53415c224a9fe455552ffc311797918064f7de))
* **mcp:** batch_replace require_change and command_position ([#1522](https://github.com/patchloom/patchloom/issues/1522)) ([0c1ac20](https://github.com/patchloom/patchloom/commit/0c1ac20861915dd037b402ac10a79aeb2b281842))
* peel runit/daemontools/s6 wrappers in command_position ([#1646](https://github.com/patchloom/patchloom/issues/1646)) ([575e568](https://github.com/patchloom/patchloom/commit/575e5685bd0068b9146f352f1f04a71ffa7f58a0))
* **plan:** accept ops as alias for operations ([#1578](https://github.com/patchloom/patchloom/issues/1578)) ([fe21341](https://github.com/patchloom/patchloom/commit/fe21341688d4d2bf81d44157a7fb38376a72f6ad))
* **plan:** require_change and command_position on replace ([#1516](https://github.com/patchloom/patchloom/issues/1516)) ([774628e](https://github.com/patchloom/patchloom/commit/774628ee591d68f56542e5ecdad7b1dea895588e))
* **replace:** peel busybox multicall applets in command_position ([#1533](https://github.com/patchloom/patchloom/issues/1533)) ([fc2125f](https://github.com/patchloom/patchloom/commit/fc2125fe16dbd5770e003b4e2e8a08968548b06b))
* **replace:** peel flock/chroot/runuser wrappers in command_position ([#1528](https://github.com/patchloom/patchloom/issues/1528)) ([e4864d2](https://github.com/patchloom/patchloom/commit/e4864d23dfb2a98c0b474ed5dda9d4a3fece4bcb))
* **replace:** peel setsid in command_position; docs CLI surface ([#1527](https://github.com/patchloom/patchloom/issues/1527)) ([465e201](https://github.com/patchloom/patchloom/commit/465e201756c0d261e12b15cea6b77b7fc483268e))


### Bug Fixes

* address GitHub AI findings (tx serialize, tidy tests) ([#1650](https://github.com/patchloom/patchloom/issues/1650)) ([86369b1](https://github.com/patchloom/patchloom/commit/86369b1d470f11ab6f6b2c5a955f111029beedca))
* **api:** command_position multi-line wrappers and timeout/nice peel ([#1511](https://github.com/patchloom/patchloom/issues/1511)) ([6b813a6](https://github.com/patchloom/patchloom/commit/6b813a69ce3face96103f35158e1d4046e51de74))
* **api:** content_edits match_count, shell flags; docs for 0.12.0 ([#1506](https://github.com/patchloom/patchloom/issues/1506)) ([e1c69e8](https://github.com/patchloom/patchloom/commit/e1c69e8b9ede4d097c81c36c53faaf31c93e6c80))
* **api:** fail closed on invalid search regex ([#1621](https://github.com/patchloom/patchloom/issues/1621)) ([d0c31d6](https://github.com/patchloom/patchloom/commit/d0c31d664a3270e1f2e5f593ae86d3fefb0bc234))
* **api:** peel eval/source for command_position ([#1514](https://github.com/patchloom/patchloom/issues/1514)) ([9d65bf9](https://github.com/patchloom/patchloom/commit/9d65bf952bb5343b47071e2f86bcb4e3a0adbcc6))
* **api:** peel exit typed errors in edit_error_kind ([#1620](https://github.com/patchloom/patchloom/issues/1620)) ([cea5313](https://github.com/patchloom/patchloom/commit/cea5313beed1675bce0f9f6553d3fdcaf86b2735))
* **api:** peel IO not_found in edit_error_kind ([#1641](https://github.com/patchloom/patchloom/issues/1641)) ([487243c](https://github.com/patchloom/patchloom/commit/487243c55eedb6ced000064951b1789015746bf6))
* **api:** peel sudo -u USER for command_position ([#1509](https://github.com/patchloom/patchloom/issues/1509)) ([e86ab87](https://github.com/patchloom/patchloom/commit/e86ab87451bf2bc15da5e8d1692fc5c15d71ad8e))
* **api:** real path in apply_content_edits_to_file diff headers ([#1502](https://github.com/patchloom/patchloom/issues/1502)) ([e90dc9b](https://github.com/patchloom/patchloom/commit/e90dc9b83e20b317ad804278a72572b1214fcf1a)), closes [#1500](https://github.com/patchloom/patchloom/issues/1500)
* **api:** type AST insert/wrap/reorder and plan filter errors ([#1605](https://github.com/patchloom/patchloom/issues/1605)) ([7591087](https://github.com/patchloom/patchloom/commit/7591087a8877ed97155397a38237e572885557f7))
* **api:** type replace/wrap validation and empty patch ([#1606](https://github.com/patchloom/patchloom/issues/1606)) ([da5d3dc](https://github.com/patchloom/patchloom/commit/da5d3dcb5bf6469eaf03f2d066517367d718eb25))
* **ast:** missing paths preserve NotFound for error_kind ([#1603](https://github.com/patchloom/patchloom/issues/1603)) ([f75d0ab](https://github.com/patchloom/patchloom/commit/f75d0ab2c852879207e2323f7b117cec5dbf0ca7))
* **ast:** preserve body gap when rewriting function signatures ([#1504](https://github.com/patchloom/patchloom/issues/1504)) ([f20123e](https://github.com/patchloom/patchloom/commit/f20123e5c685a3e7264b1a5e129d74dae32741ba)), closes [#1503](https://github.com/patchloom/patchloom/issues/1503)
* **batch:** parse line errors are typed ParseErrorError ([#1608](https://github.com/patchloom/patchloom/issues/1608)) ([e0b682b](https://github.com/patchloom/patchloom/commit/e0b682b920247a31706efab7e7a28e41ce4ec8ec))
* **cli:** --contain JSON error_kind invalid_input ([#1576](https://github.com/patchloom/patchloom/issues/1576)) ([d5b9835](https://github.com/patchloom/patchloom/commit/d5b983538f7643f8c575ad86adabd9f427d7b253))
* **cli:** --cwd missing or non-dir is invalid_input ([#1599](https://github.com/patchloom/patchloom/issues/1599)) ([be49c4f](https://github.com/patchloom/patchloom/commit/be49c4f879e1055118a0dabfe6148b0470a89a1a))
* **cli:** batch parse_error error_kind with exit 4 ([#1570](https://github.com/patchloom/patchloom/issues/1570)) ([867c174](https://github.com/patchloom/patchloom/commit/867c174e6c57021c8f65bde1b010fc90fa8c3f21))
* **cli:** compact clap usage messages under JSON ([#1577](https://github.com/patchloom/patchloom/issues/1577)) ([b24d102](https://github.com/patchloom/patchloom/commit/b24d102d4210ad9d21be4266d1cdaaf96b52613d))
* **cli:** doc type_error error_kind and wrapper docs ([#1560](https://github.com/patchloom/patchloom/issues/1560)) ([c379c6e](https://github.com/patchloom/patchloom/commit/c379c6ec99210b654ad68994e438e33866965316))
* **cli:** emit error_kind no_matches on doc/md/AST JSON exits ([#1550](https://github.com/patchloom/patchloom/issues/1550)) ([8786cfc](https://github.com/patchloom/patchloom/commit/8786cfcb79097037cc79ea54d8833300d80748e8))
* **cli:** empty paths and unsupported init shell are invalid_input ([#1600](https://github.com/patchloom/patchloom/issues/1600)) ([32771da](https://github.com/patchloom/patchloom/commit/32771da6c584fef6d1354aada71e4c39b628d676))
* **cli:** error_kind on file op validation failures ([#1562](https://github.com/patchloom/patchloom/issues/1562)) ([15b07ec](https://github.com/patchloom/patchloom/commit/15b07ecbdddfeaad9748f4167ed11fc74caecc27))
* **cli:** explain parse_error error_kind and exit 4 ([#1573](https://github.com/patchloom/patchloom/issues/1573)) ([4d83f92](https://github.com/patchloom/patchloom/commit/4d83f924cf14e4b12c0e686f3858886ce4ad978b))
* **cli:** files-from all-missing targets return not_found ([#1582](https://github.com/patchloom/patchloom/issues/1582)) ([eca3920](https://github.com/patchloom/patchloom/commit/eca3920037f81d428906d43ff967452788595441))
* **cli:** identity replace is success, not no matches ([#1530](https://github.com/patchloom/patchloom/issues/1530)) ([81d19e6](https://github.com/patchloom/patchloom/commit/81d19e6e2906adb202731701c3ef193693ca12f0))
* **cli:** JSON usage errors and replace empty-pattern wording ([#1575](https://github.com/patchloom/patchloom/issues/1575)) ([0085b12](https://github.com/patchloom/patchloom/commit/0085b12e4b378c2ae07e741ee22e47232568aa42))
* **cli:** map clap usage errors to exit 1 not 2 ([#1574](https://github.com/patchloom/patchloom/issues/1574)) ([7ebeccc](https://github.com/patchloom/patchloom/commit/7ebeccc4c20cec68d42798998331223a532cc329))
* **cli:** map engine IO NotFound to JSON not_found ([#1584](https://github.com/patchloom/patchloom/issues/1584)) ([eb0e113](https://github.com/patchloom/patchloom/commit/eb0e113dada64a56eb5f79a6e1c8ceb6e69bbe0f))
* **cli:** md validation JSON invalid_input error_kind ([#1567](https://github.com/patchloom/patchloom/issues/1567)) ([12feea3](https://github.com/patchloom/patchloom/commit/12feea3be421626d19d5c11fb8760631127f8aab))
* **cli:** missing path roots return not_found ([#1581](https://github.com/patchloom/patchloom/issues/1581)) ([1039ffe](https://github.com/patchloom/patchloom/commit/1039ffe67a5dcf688d5ef3a5596f4bb2992edb17))
* **cli:** more JSON error_kind for read, status, doc, AST ([#1569](https://github.com/patchloom/patchloom/issues/1569)) ([a188089](https://github.com/patchloom/patchloom/commit/a188089064a75bf2725928dfafa247bb3a7fb9fb))
* **cli:** patch IO and binary rename invalid_input error_kind ([#1571](https://github.com/patchloom/patchloom/issues/1571)) ([606db63](https://github.com/patchloom/patchloom/commit/606db632985e1dbe133dce07d77fe7825650a798))
* **cli:** replace validation JSON invalid_input error_kind ([#1568](https://github.com/patchloom/patchloom/issues/1568)) ([af9bf59](https://github.com/patchloom/patchloom/commit/af9bf59334f4430782fb7b6560d6c371234f1251))
* **cli:** search validation JSON invalid_input error_kind ([#1566](https://github.com/patchloom/patchloom/issues/1566)) ([5f6bec1](https://github.com/patchloom/patchloom/commit/5f6bec1d55192dda89dd54523294d3e656aa65b4))
* **cli:** set error_kind on patch --json parse/stale/conflict ([#1551](https://github.com/patchloom/patchloom/issues/1551)) ([e9ac6a6](https://github.com/patchloom/patchloom/commit/e9ac6a604ffc13a5543a39ade2501373ff22714f))
* **cli:** set error_kind on replace --json no_matches/ambiguous ([#1548](https://github.com/patchloom/patchloom/issues/1548)) ([c61e44c](https://github.com/patchloom/patchloom/commit/c61e44c3377b350a54b514abd9bd890a285ff8e6))
* **cli:** set error_kind on search --json no_matches ([#1549](https://github.com/patchloom/patchloom/issues/1549)) ([fc8ab51](https://github.com/patchloom/patchloom/commit/fc8ab5141099a49aac7d3a6d0bb73e37ac81f7f5))
* **cli:** tidy dual-flag invalid_input error_kind ([#1564](https://github.com/patchloom/patchloom/issues/1564)) ([1587713](https://github.com/patchloom/patchloom/commit/1587713988ea001d97886cdef7c4917ac7109886))
* **cli:** type doc merge, status, and AST grammar errors ([#1607](https://github.com/patchloom/patchloom/issues/1607)) ([2947c95](https://github.com/patchloom/patchloom/commit/2947c95131f85e4b00c5aafa7bd1f1a2e7b9dddc))
* **cli:** type more AST, plan, and write-policy errors ([#1609](https://github.com/patchloom/patchloom/issues/1609)) ([67f6a94](https://github.com/patchloom/patchloom/commit/67f6a949d2a4878a9aab2ce1fd7ae37310407c92))
* **cli:** type post-write --format failures as format_failed ([#1626](https://github.com/patchloom/patchloom/issues/1626)) ([e2ee085](https://github.com/patchloom/patchloom/commit/e2ee085a6d3438cc04dae555fd6c78e7c8cb5d68))
* **cli:** typed JSON dispatch errors and more shell wrappers ([#1561](https://github.com/patchloom/patchloom/issues/1561)) ([5f4766c](https://github.com/patchloom/patchloom/commit/5f4766cc1207f9cec817fe79fad63fce1fa2baed))
* **doc:** malformed documents exit 4 with parse_error ([#1595](https://github.com/patchloom/patchloom/issues/1595)) ([aa23fd3](https://github.com/patchloom/patchloom/commit/aa23fd31a5ad9e068df300fc3ecf33a7f91c1f27))
* **doc:** more selector type mismatches are type_error ([#1592](https://github.com/patchloom/patchloom/issues/1592)) ([358f024](https://github.com/patchloom/patchloom/commit/358f0248c611c3c42def7f619b67a189cd4d86ab))
* **doc:** preserve format_failed error_kind on post-write format ([#1634](https://github.com/patchloom/patchloom/issues/1634)) ([21c81b6](https://github.com/patchloom/patchloom/commit/21c81b67c5997c74aa3f3b6007800b1dae8bac25))
* **doc:** type navigate selector mistakes for agents ([#1601](https://github.com/patchloom/patchloom/issues/1601)) ([4f00d1d](https://github.com/patchloom/patchloom/commit/4f00d1d79104470906ec46f5584bf56f76fede1d))
* **doc:** type remaining navigate ok_or_else errors ([#1602](https://github.com/patchloom/patchloom/issues/1602)) ([2ea1a7b](https://github.com/patchloom/patchloom/commit/2ea1a7bd96dd83d9c27f74c6b7746ef7aef256d4))
* **doc:** type YAML multiline string escape failures ([#1619](https://github.com/patchloom/patchloom/issues/1619)) ([fcb8f81](https://github.com/patchloom/patchloom/commit/fcb8f8197c8e0c2c824ebf8a6bd2cb17b25ba4c9))
* **doc:** type YAML/TOML preserve-path serialize and re-parse errors ([#1618](https://github.com/patchloom/patchloom/issues/1618)) ([2138c87](https://github.com/patchloom/patchloom/commit/2138c87e9b8e953562b23b8466ef165b466105ef))
* fail-closed structured JSON on serialize error ([#1652](https://github.com/patchloom/patchloom/issues/1652)) ([378c687](https://github.com/patchloom/patchloom/commit/378c687c630d140427a14db69740ce4bdd6a928e))
* **read:** parse_line_range raises InvalidInputError ([#1604](https://github.com/patchloom/patchloom/issues/1604)) ([c879b55](https://github.com/patchloom/patchloom/commit/c879b55424a8da40b7f600b71901c227c7b7d172))
* **search:** assert-count JSON error_kind changes_detected ([#1648](https://github.com/patchloom/patchloom/issues/1648)) ([27867eb](https://github.com/patchloom/patchloom/commit/27867eb5ff72a218ede818b15dbdc4c89e8f8fd7))
* **search:** type invalid regex as invalid_input ([#1622](https://github.com/patchloom/patchloom/issues/1622)) ([c01a52c](https://github.com/patchloom/patchloom/commit/c01a52cfb4cbf0ace970d337bdb460c1be63a4fc))
* shell container wrappers and undo JSON error_kind ([#1559](https://github.com/patchloom/patchloom/issues/1559)) ([9d3092d](https://github.com/patchloom/patchloom/commit/9d3092d2348a97688af61377f0a878c03a4e08a3))
* **shell_token:** peel CI isolation and priority wrappers ([#1547](https://github.com/patchloom/patchloom/issues/1547)) ([c7aa0fb](https://github.com/patchloom/patchloom/commit/c7aa0fb64cdb1de651f5e591feb3c8ce17777d17))
* **shell_token:** peel env --chdir; note env --unset in RELEASE_NOTES ([#1545](https://github.com/patchloom/patchloom/issues/1545)) ([85e0b54](https://github.com/patchloom/patchloom/commit/85e0b54b48b65063db4012aac6fc427db6786924))
* **shell_token:** peel env --unset; docs identity JSON ([#1544](https://github.com/patchloom/patchloom/issues/1544)) ([8a8472d](https://github.com/patchloom/patchloom/commit/8a8472d365540be78de5a0726038f578c107f095))
* **shell_token:** peel timeout --kill-after stacked durations ([#1546](https://github.com/patchloom/patchloom/issues/1546)) ([42b2c54](https://github.com/patchloom/patchloom/commit/42b2c54370e6e9d802ea04d82569e8e0b94bd0c2))
* **shell_token:** peel unit/sandbox and run0 wrappers ([#1557](https://github.com/patchloom/patchloom/issues/1557)) ([d97d9a8](https://github.com/patchloom/patchloom/commit/d97d9a8a139a5e0322aebd73db2e04c920da3ed4))
* **tx:** AST extract already_exists and rewrite invalid_input ([#1596](https://github.com/patchloom/patchloom/issues/1596)) ([43bce76](https://github.com/patchloom/patchloom/commit/43bce76ac23b80ab58f044cd0044207f98205ccb))
* **tx:** create/rename conflicts set already_exists ([#1587](https://github.com/patchloom/patchloom/issues/1587)) ([81423e1](https://github.com/patchloom/patchloom/commit/81423e1330537c0a52504abf82f5cf58ad9013b4))
* **tx:** doc type mismatches set type_error ([#1590](https://github.com/patchloom/patchloom/issues/1590)) ([7e650b9](https://github.com/patchloom/patchloom/commit/7e650b957e80ee41539e99affb23f99ec6c05109))
* **tx:** doc.delete_where non-array is type_error ([#1591](https://github.com/patchloom/patchloom/issues/1591)) ([e48dbc1](https://github.com/patchloom/patchloom/commit/e48dbc1e67e36c277f3f3f2981aaa6e527eb89eb))
* **tx:** file append/prepend/delete missing are not_found ([#1586](https://github.com/patchloom/patchloom/issues/1586)) ([3da370e](https://github.com/patchloom/patchloom/commit/3da370e03edeb871a2b40307f657e6afa986eee3))
* **tx:** invalid_input for runtime flags and doc extensions ([#1594](https://github.com/patchloom/patchloom/issues/1594)) ([9077014](https://github.com/patchloom/patchloom/commit/907701434d3521a4b8d62d848c965bc0a2262420))
* **tx:** map IO NotFound to error_kind not_found ([#1585](https://github.com/patchloom/patchloom/issues/1585)) ([69ed612](https://github.com/patchloom/patchloom/commit/69ed612e1666763ff7813532c8c4ff5eb46b90a4))
* **tx:** mid-plan delete is not_found; contain escapes invalid_input ([#1597](https://github.com/patchloom/patchloom/issues/1597)) ([070f8ff](https://github.com/patchloom/patchloom/commit/070f8ff790e0e34acbda8e9f93ddf7b11bcd52a2))
* **tx:** non-file targets invalid_input; path_err keeps NotFound ([#1588](https://github.com/patchloom/patchloom/issues/1588)) ([909e478](https://github.com/patchloom/patchloom/commit/909e478ee30005ba06757f888e3bd4b013ea8d4b))
* **tx:** patch.apply merge conflicts exit 8 with conflicts kind ([#1593](https://github.com/patchloom/patchloom/issues/1593)) ([cdf73f4](https://github.com/patchloom/patchloom/commit/cdf73f4055721e600453774e6975fa319f161262))
* **tx:** preview JSON status is changes_detected ([#1649](https://github.com/patchloom/patchloom/issues/1649)) ([82206fa](https://github.com/patchloom/patchloom/commit/82206faba2bf60734da05c05922ef60ba993c37f))
* **tx:** search assert_count mismatch is changes_detected ([#1598](https://github.com/patchloom/patchloom/issues/1598)) ([9bf52ac](https://github.com/patchloom/patchloom/commit/9bf52acd433e9b7864a6f2010713350c2d77959b))
* **tx:** unique and require_change map to exit 5/3 ([#1539](https://github.com/patchloom/patchloom/issues/1539)) ([504fd7d](https://github.com/patchloom/patchloom/commit/504fd7db9a7c16e36db83801beb2c124c0b0acb4))
* verify attr filter and fail-closed JSON formatters ([#1653](https://github.com/patchloom/patchloom/issues/1653)) ([973a714](https://github.com/patchloom/patchloom/commit/973a7143ab8ac8d23c2f0d85d4339a5166938bf1))


### Performance Improvements

* **ast:** parallelize multi-file rename match pre-scan ([#1612](https://github.com/patchloom/patchloom/issues/1612)) ([5bcfd74](https://github.com/patchloom/patchloom/commit/5bcfd74dbdcc17b2fc44649710e3ae307a8baa23))
* **ast:** parallelize reverse deps project scan ([#1613](https://github.com/patchloom/patchloom/issues/1613)) ([a180762](https://github.com/patchloom/patchloom/commit/a180762aa8dad1a98931a536163d20f559cd429f))
* **mcp:** parallelize multi-file AST rename pre-scan ([#1614](https://github.com/patchloom/patchloom/issues/1614)) ([cbc0624](https://github.com/patchloom/patchloom/commit/cbc062459a3e824094266392b31fd25a7ecfb856))

## [0.11.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.10.0...patchloom-v0.11.0) (2026-07-09)


### Features

* library embedder gaps for LLM agents ([#1459](https://github.com/patchloom/patchloom/issues/1459)) ([#1461](https://github.com/patchloom/patchloom/issues/1461)) ([37d4573](https://github.com/patchloom/patchloom/commit/37d457373af009d293653a7440c23c860d923e7a))


### Bug Fixes

* accept singular file alias on batch_replace and batch_tidy ([#1471](https://github.com/patchloom/patchloom/issues/1471)) ([15ace34](https://github.com/patchloom/patchloom/commit/15ace34ebe97bfcf21345f807d3f6ab2f43a8b61))
* batch unknown-op suggestions and deny.toml licenses ([#1483](https://github.com/patchloom/patchloom/issues/1483)) ([b61dde5](https://github.com/patchloom/patchloom/commit/b61dde51d653385006d0c174906ac76db499b2d7))
* enforce --contain on ast list/deps/map/diff ([#1456](https://github.com/patchloom/patchloom/issues/1456)) ([feeace7](https://github.com/patchloom/patchloom/commit/feeace75e41c259ccb259fdb3dcfde3e971f73e0))
* files-from no-match scope + concurrent inventory ([#1476](https://github.com/patchloom/patchloom/issues/1476)) ([3160c62](https://github.com/patchloom/patchloom/commit/3160c622e8a9f92313327ee361833d01efab3565))
* honor contained plan.cwd in MCP execute_plan ([#1466](https://github.com/patchloom/patchloom/issues/1466)) ([7d4691f](https://github.com/patchloom/patchloom/commit/7d4691fe87cfb080c2cd9f5353a810440e6bb98f))
* map AST/md/doc no-match in tx to exit 3 with detail ([#1462](https://github.com/patchloom/patchloom/issues/1462)) ([db9ddb7](https://github.com/patchloom/patchloom/commit/db9ddb77c91884dab1311a2b7da05ed7b7fccb84))
* MPI cycle improve (regex, lychee 5xx, yaml escape) ([#1487](https://github.com/patchloom/patchloom/issues/1487)) ([8a48e06](https://github.com/patchloom/patchloom/commit/8a48e063e188cf32de1af9e6f28b00bbdec2a1b3))
* normalize absolute paths in unified-diff headers ([#1481](https://github.com/patchloom/patchloom/issues/1481)) ([44cb0e6](https://github.com/patchloom/patchloom/commit/44cb0e6943a3ac112abbdb3b9d89a30fd9f4fe51))
* omit .patchloom backups from status output ([#1478](https://github.com/patchloom/patchloom/issues/1478)) ([bd71231](https://github.com/patchloom/patchloom/commit/bd712316bb21e847ae9bac6599e8a500c2a6016e))
* preserve NoMatchError through error chain wrapping ([#1464](https://github.com/patchloom/patchloom/issues/1464)) ([9b6b519](https://github.com/patchloom/patchloom/commit/9b6b5196a9a92b256c62c1e43508435416e5c30f))
* reject empty and whitespace-only path arguments ([#1460](https://github.com/patchloom/patchloom/issues/1460)) ([cee8c66](https://github.com/patchloom/patchloom/commit/cee8c66ae305250a200a4e85a799ec67e503b866))
* reject empty/whitespace path alias and plan.cwd on MCP ([#1472](https://github.com/patchloom/patchloom/issues/1472)) ([b3aac77](https://github.com/patchloom/patchloom/commit/b3aac771f7f8596ec6fc6f5f9a0cd1354da1eb2c))
* search_files path alias and AST concurrent-write guidance ([#1469](https://github.com/patchloom/patchloom/issues/1469)) ([d6f8510](https://github.com/patchloom/patchloom/commit/d6f8510413e5a560784c29e6a261020a3c7eb224))
* strip all leading slashes in diff header paths ([#1482](https://github.com/patchloom/patchloom/issues/1482)) ([6bb64d6](https://github.com/patchloom/patchloom/commit/6bb64d6ada9f0af9488608102ae2c5fa7c6a93a5)), closes [#1480](https://github.com/patchloom/patchloom/issues/1480)

## [0.10.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.9.0...patchloom-v0.10.0) (2026-07-07)


### Features

* add --contain for optional CLI workspace path guarding ([#1407](https://github.com/patchloom/patchloom/issues/1407)) ([4a088a4](https://github.com/patchloom/patchloom/commit/4a088a45c5cc1c4c1bf6a94613843ec37d070048))
* MCP/tx doc delete mutation summary (changed/removed) ([#1441](https://github.com/patchloom/patchloom/issues/1441)) ([7eb321f](https://github.com/patchloom/patchloom/commit/7eb321f6f4affd14c71cc00e28988d1c55520213)), closes [#1439](https://github.com/patchloom/patchloom/issues/1439)
* post-rewrite follow-ups (MCP meta, EngineContext, break shims) ([#1387](https://github.com/patchloom/patchloom/issues/1387)) ([43ed437](https://github.com/patchloom/patchloom/commit/43ed437bc4f3711ac7d05e1e880610238f4a42bb)), closes [#1383](https://github.com/patchloom/patchloom/issues/1383) [#1384](https://github.com/patchloom/patchloom/issues/1384) [#1385](https://github.com/patchloom/patchloom/issues/1385) [#1386](https://github.com/patchloom/patchloom/issues/1386) [#1382](https://github.com/patchloom/patchloom/issues/1382)


### Bug Fixes

* --contain AllowIfContained, stdin files-from errors, meta tests ([#1452](https://github.com/patchloom/patchloom/issues/1452)) ([fff5a5e](https://github.com/patchloom/patchloom/commit/fff5a5ec8f2988c678cea3ef0823c8a15ca5ac96)), closes [#1449](https://github.com/patchloom/patchloom/issues/1449) [#1450](https://github.com/patchloom/patchloom/issues/1450) [#1451](https://github.com/patchloom/patchloom/issues/1451)
* --files-from under --cwd; contain before append exists-check ([#1419](https://github.com/patchloom/patchloom/issues/1419)) ([42aa936](https://github.com/patchloom/patchloom/commit/42aa9367c90b6ee08aa0e123f59413dc0da44610))
* accept "command" as alias for "cmd" in tx format/validate steps ([#1323](https://github.com/patchloom/patchloom/issues/1323)) ([d515bc0](https://github.com/patchloom/patchloom/commit/d515bc0244048fb5099c9ab007ebc31e52b0c4dd))
* accept value= predicate for scalar doc delete-where ([#1425](https://github.com/patchloom/patchloom/issues/1425)) ([7cc3e08](https://github.com/patchloom/patchloom/commit/7cc3e0884f693013b980a0f1d81364daa117378c))
* add test-library-hygiene to make check and fix CONTRIBUTING.md stale reference ([#1364](https://github.com/patchloom/patchloom/issues/1364)) ([d0b93e3](https://github.com/patchloom/patchloom/commit/d0b93e3987736f379db88a8da431e77a7d38f96c))
* agent-friendly replace aliases and patch - stdin ([#1423](https://github.com/patchloom/patchloom/issues/1423)) ([2154174](https://github.com/patchloom/patchloom/commit/2154174605296fb1e569c7d4e5a57f60bc379c64))
* agent-rules --json now emits structured JSON output ([#1322](https://github.com/patchloom/patchloom/issues/1322)) ([92ef0fd](https://github.com/patchloom/patchloom/commit/92ef0fd6692e8ad35b68c90c9320386b52083569))
* apply --contain to meta-input paths (plans, ops, files-from) ([#1447](https://github.com/patchloom/patchloom/issues/1447)) ([b1ed678](https://github.com/patchloom/patchloom/commit/b1ed67855a3049193b6494116f30ebaaae257faa))
* ast and doc commands emit JSON on no-match paths ([#1321](https://github.com/patchloom/patchloom/issues/1321)) ([f2ae6df](https://github.com/patchloom/patchloom/commit/f2ae6df6a4ee20f4538a5b760d9a3c7b35c4f499))
* ast read and md dedupe-headings exit code consistency ([#1347](https://github.com/patchloom/patchloom/issues/1347)) ([86aa80a](https://github.com/patchloom/patchloom/commit/86aa80aae1f769d0cff09ce670569161d7a73a8c))
* ast rename NoMatchError handler, dedup run_context_replace, add integration tests ([#1335](https://github.com/patchloom/patchloom/issues/1335)) ([83aace4](https://github.com/patchloom/patchloom/commit/83aace4352d44235c7e2112d4350fea7a5f8a20d))
* bounded regex compilation, ast quiet/JSON guards, error-path tests ([#1357](https://github.com/patchloom/patchloom/issues/1357)) ([55aa095](https://github.com/patchloom/patchloom/commit/55aa0957da0554fc7d144c02b5766ff55421ced5))
* box plan validation errors and strengthen verify/explain asserts ([#1403](https://github.com/patchloom/patchloom/issues/1403)) ([6225faf](https://github.com/patchloom/patchloom/commit/6225faf04cadafde511af6f1bc97af0d36a0deb4))
* bump crossbeam-epoch for RUSTSEC-2026-0204 ([#1438](https://github.com/patchloom/patchloom/issues/1438)) ([c34ba23](https://github.com/patchloom/patchloom/commit/c34ba231f8623f5610551ad0df70df47fe9b741d))
* clarify doc.update uses selector predicates, not --where ([#1431](https://github.com/patchloom/patchloom/issues/1431)) ([1664b32](https://github.com/patchloom/patchloom/commit/1664b328cf5a09951bc705d3d855e5bd979fce22))
* close PathGuard containment gaps in PatchApply and glob-replace ([#1367](https://github.com/patchloom/patchloom/issues/1367)) ([49057d8](https://github.com/patchloom/patchloom/commit/49057d86faaf506b326c410c719b9c7c9f3676d7)), closes [#1363](https://github.com/patchloom/patchloom/issues/1363) [#1361](https://github.com/patchloom/patchloom/issues/1361)
* complete CLI --contain for all write paths ([#1410](https://github.com/patchloom/patchloom/issues/1410)) ([7a242fe](https://github.com/patchloom/patchloom/commit/7a242fe1edafcd26916d8a98bd5427cf85f65610))
* context-based replace now respects --json/--jsonl flags ([#1317](https://github.com/patchloom/patchloom/issues/1317)) ([c689b88](https://github.com/patchloom/patchloom/commit/c689b88e5ccae5aba7335b8ea265f460c5fee48b))
* context-replace emits ok:false on no-match and guards stderr ([#1365](https://github.com/patchloom/patchloom/issues/1365)) ([02f6148](https://github.com/patchloom/patchloom/commit/02f61480a80f279f9a0ae7f85502d48f7d0a51d7))
* correct JSON ok field semantics and assertion quality ([#1360](https://github.com/patchloom/patchloom/issues/1360)) ([9456580](https://github.com/patchloom/patchloom/commit/94565807cc0985356dd672056a78ad8ded5187fb))
* doc key alias + ast rename usage hint from runtime scenarios ([#1420](https://github.com/patchloom/patchloom/issues/1420)) ([cfd6057](https://github.com/patchloom/patchloom/commit/cfd6057e1dfd1afeddce4a2ad3d3b136f01d92cc))
* drop indent spaces from feature-gated MCP instruction lines ([#1399](https://github.com/patchloom/patchloom/issues/1399)) ([e946a80](https://github.com/patchloom/patchloom/commit/e946a808df6a85750c12841749fc99049c5bddef))
* emit stderr for no-match errors in text mode and add test coverage ([#1338](https://github.com/patchloom/patchloom/issues/1338)) ([ecfc12a](https://github.com/patchloom/patchloom/commit/ecfc12a4ca318f1bfcc492130baaec6b4b18a208))
* emit stderr for silent no-match errors in doc and undo commands ([#1340](https://github.com/patchloom/patchloom/issues/1340), [#1341](https://github.com/patchloom/patchloom/issues/1341)) ([#1343](https://github.com/patchloom/patchloom/issues/1343)) ([694eb71](https://github.com/patchloom/patchloom/commit/694eb71758162c0645fa186976972488aa5e7eff))
* emit stderr for silent no-match/ambiguous errors in replace and search commands ([#1344](https://github.com/patchloom/patchloom/issues/1344)) ([387acfa](https://github.com/patchloom/patchloom/commit/387acfa0bfac2aff5eb3e50b7eb41e9d889ba137))
* enforce --contain on CLI reads as well as writes ([#1417](https://github.com/patchloom/patchloom/issues/1417)) ([93a64a7](https://github.com/patchloom/patchloom/commit/93a64a7381fe7daf38e857bc8dd524fe7bc45cfa))
* enforce --contain on CLI tx plans ([#1412](https://github.com/patchloom/patchloom/issues/1412)) ([40aad6a](https://github.com/patchloom/patchloom/commit/40aad6a5a3d1b62b2785da1f9b18b4a2111a0898))
* expose schema --tier as clap ValueEnum ([#1427](https://github.com/patchloom/patchloom/issues/1427)) ([ecadbe1](https://github.com/patchloom/patchloom/commit/ecadbe18e256e86a468d58dd5fea1eb6ae2897d6))
* fail --contain replace before scanning escaped paths ([#1415](https://github.com/patchloom/patchloom/issues/1415)) ([b4d3c55](https://github.com/patchloom/patchloom/commit/b4d3c557326ef84160a6cba67b2c7420d62be3e0))
* fallback replace path ignores context/fuzzy + Unicode tests ([#1316](https://github.com/patchloom/patchloom/issues/1316)) ([f757ed2](https://github.com/patchloom/patchloom/commit/f757ed227b5e3aaa75ffdff5f72aaf1f62d99fb4))
* feature-gate custom MCP inventory to match list_tools ([#1395](https://github.com/patchloom/patchloom/issues/1395)) ([c7ea986](https://github.com/patchloom/patchloom/commit/c7ea986629ce13340474354f04d00d7fdcc8401c))
* feature-gate verify HashMap and VerifyResult for non-ast builds ([#1398](https://github.com/patchloom/patchloom/issues/1398)) ([9b312c6](https://github.com/patchloom/patchloom/commit/9b312c67e8d928c2643d1e79d7533b076b1d775a))
* honor --contain for --files-from path lists ([#1418](https://github.com/patchloom/patchloom/issues/1418)) ([d029776](https://github.com/patchloom/patchloom/commit/d029776e1235e9312accead653b931be25a29b99))
* JSON output for no-match cases across doc, search, and md commands ([#1319](https://github.com/patchloom/patchloom/issues/1319)) ([279c2a3](https://github.com/patchloom/patchloom/commit/279c2a392249c76e2bad9aa00b1bee4e7920be81))
* MCP key alias for doc params; lock ast rename order hint ([#1421](https://github.com/patchloom/patchloom/issues/1421)) ([5f0e8b3](https://github.com/patchloom/patchloom/commit/5f0e8b3fd2f54908bf0205c4fb90b674c72a85dd))
* migrate redundant json flag checks to emit_json() return value ([#1326](https://github.com/patchloom/patchloom/issues/1326)) ([2cfc157](https://github.com/patchloom/patchloom/commit/2cfc15732feb0b9064d421f911b6ea0cc4b8c935)), closes [#1324](https://github.com/patchloom/patchloom/issues/1324)
* MPI rotation - exit codes, JSON output, backup perms, editorconfig, docs ([#1354](https://github.com/patchloom/patchloom/issues/1354)) ([49b3188](https://github.com/patchloom/patchloom/commit/49b31888a446b8a75489da55a99b1ba636a24ca5))
* name workspace root in --contain escape errors ([#1414](https://github.com/patchloom/patchloom/issues/1414)) ([a862a54](https://github.com/patchloom/patchloom/commit/a862a542ba23ef073ddfc2c7121805a99d805504))
* patch apply now respects --json/--jsonl flags ([#1318](https://github.com/patchloom/patchloom/issues/1318)) ([5b615f3](https://github.com/patchloom/patchloom/commit/5b615f3611b572665fe7cc95381e283b58fd653d))
* pin release installers and doc write JSON mutation summary ([#1437](https://github.com/patchloom/patchloom/issues/1437)) ([90b91e0](https://github.com/patchloom/patchloom/commit/90b91e06bab28826b5a9195c26ce69e19628b254)), closes [#1433](https://github.com/patchloom/patchloom/issues/1433) [#1434](https://github.com/patchloom/patchloom/issues/1434) [#1436](https://github.com/patchloom/patchloom/issues/1436)
* preserve inline table format during doc set operations ([#1328](https://github.com/patchloom/patchloom/issues/1328)) ([404c9b4](https://github.com/patchloom/patchloom/commit/404c9b4655f963c9b10f71b1ce34e0e85f8c47ae))
* replace string-based error classification with typed NoMatchError ([#1334](https://github.com/patchloom/patchloom/issues/1334)) ([004e78e](https://github.com/patchloom/patchloom/commit/004e78e4c670db63b3bf038a54a535aa91faf391)), closes [#1331](https://github.com/patchloom/patchloom/issues/1331) [#1332](https://github.com/patchloom/patchloom/issues/1332) [#1333](https://github.com/patchloom/patchloom/issues/1333)
* resolve batch and patch input paths under --cwd ([#1444](https://github.com/patchloom/patchloom/issues/1444)) ([9be6776](https://github.com/patchloom/patchloom/commit/9be6776e9a5f93650f2b05bb5e1116a3eb4df157))
* resolve bugs [#1349](https://github.com/patchloom/patchloom/issues/1349) and [#1350](https://github.com/patchloom/patchloom/issues/1350) (exit code and .patchloom exclusion) ([#1352](https://github.com/patchloom/patchloom/issues/1352)) ([a9d63fc](https://github.com/patchloom/patchloom/commit/a9d63fc68a2e55fa3a39e260d25f6823a819d0a9))
* return CHANGES_DETECTED exit code in default preview mode ([#1345](https://github.com/patchloom/patchloom/issues/1345)) ([519401c](https://github.com/patchloom/patchloom/commit/519401c24574199c01f2fc8c030ed6bc177cbd43))
* return CHANGES_DETECTED in default mode for tx, batch, and patch commands ([#1346](https://github.com/patchloom/patchloom/issues/1346)) ([d5693b3](https://github.com/patchloom/patchloom/commit/d5693b311929a6abe374025092fdd932c970e47a))
* split WritePolicyOverride into config vs plan types ([#1356](https://github.com/patchloom/patchloom/issues/1356)) ([aaff244](https://github.com/patchloom/patchloom/commit/aaff2441016337e9f14a302f826431e51b25b832))
* tidy fix defaults match tidy check issues ([#1424](https://github.com/patchloom/patchloom/issues/1424)) ([1f2d2a9](https://github.com/patchloom/patchloom/commit/1f2d2a95adea35c3a1c7b05de9d236418f035fc3))
* undo --json emits structured output on apply success and no-sessions ([#1320](https://github.com/patchloom/patchloom/issues/1320)) ([ae6c4f4](https://github.com/patchloom/patchloom/commit/ae6c4f4a083002b6b95f34ebe801a0b299f98c42))
* use numeric version in agent-rules tx plan example ([#1329](https://github.com/patchloom/patchloom/issues/1329)) ([0e13065](https://github.com/patchloom/patchloom/commit/0e1306568caa949973e9b309c87738aac5bbf771))
* use old/new consistently for ast rename across surfaces ([#1426](https://github.com/patchloom/patchloom/issues/1426)) ([35b5268](https://github.com/patchloom/patchloom/commit/35b5268f389c2b971c293ffa8fc33c36dc63944d))
* wire context anchoring through replace_in_content fuzzy fallback ([#1312](https://github.com/patchloom/patchloom/issues/1312)) ([5a9a7e5](https://github.com/patchloom/patchloom/commit/5a9a7e5c664a843f368c5b66fe0f28138eb490f5))
* write_dispatch preview mode returns exit 0 instead of exit 2 ([#1348](https://github.com/patchloom/patchloom/issues/1348)) ([f419929](https://github.com/patchloom/patchloom/commit/f41992993e4a5d2eda49ac8305f150f53a16bf9f))

## [0.9.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.8.0...patchloom-v0.9.0) (2026-07-01)


### Features

* add --before-context/--after-context to CLI replace and new prepend command ([#1290](https://github.com/patchloom/patchloom/issues/1290)) ([8a576dd](https://github.com/patchloom/patchloom/commit/8a576dd55d106de02b6dd2bd6152d7f9f38f125b))
* add fuzzy fallback to replace_in_content ([#1286](https://github.com/patchloom/patchloom/issues/1286)) ([#1292](https://github.com/patchloom/patchloom/issues/1292)) ([a7c009e](https://github.com/patchloom/patchloom/commit/a7c009e4dbbdecabc6ec813269100d3dc86ba2d2))
* implement agent-host issues [#1287](https://github.com/patchloom/patchloom/issues/1287), [#1288](https://github.com/patchloom/patchloom/issues/1288), [#1289](https://github.com/patchloom/patchloom/issues/1289) ([#1291](https://github.com/patchloom/patchloom/issues/1291)) ([d24dd45](https://github.com/patchloom/patchloom/commit/d24dd45b1b6a08667acd998d30628a4171e77bdc))


### Bug Fixes

* md replace-section preserves blank line before next heading ([#1307](https://github.com/patchloom/patchloom/issues/1307)) ([2ff0f3b](https://github.com/patchloom/patchloom/commit/2ff0f3b5ae79e6c861de1474c516667eee196266))
* md replace-section strips duplicate heading from replacement content ([#1306](https://github.com/patchloom/patchloom/issues/1306)) ([ad9c7d2](https://github.com/patchloom/patchloom/commit/ad9c7d260389c8eac745b9e7c8ff7355a1933383))
* patch check creation, doc move descendant guard, replace exit codes ([#1297](https://github.com/patchloom/patchloom/issues/1297)) ([1760381](https://github.com/patchloom/patchloom/commit/17603818a5dc9e60fd9a4722b801d09555d955d4))
* path traversal bypass in containment and bare unwrap cleanup ([#1296](https://github.com/patchloom/patchloom/issues/1296)) ([19015f9](https://github.com/patchloom/patchloom/commit/19015f94364b500c1f5623f013910eb8b777ad8c))
* preserve YAML quote styles on doc set scalar updates ([#1283](https://github.com/patchloom/patchloom/issues/1283)) ([18f7482](https://github.com/patchloom/patchloom/commit/18f7482708ade50097f699d7d788b5b507892080))
* wire by_extension formatters from .patchloom.toml to run_format_command ([#1285](https://github.com/patchloom/patchloom/issues/1285)) ([b29b78b](https://github.com/patchloom/patchloom/commit/b29b78b0d9a425ce09f2c8486308770173285554))

## [0.8.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.7.0...patchloom-v0.8.0) (2026-07-01)


### Features

* **api:** add text_diff API and unique mode + match_count for replace ([#1269](https://github.com/patchloom/patchloom/issues/1269)) ([519f674](https://github.com/patchloom/patchloom/commit/519f6745619ba3c2fda7d4e168b911a2c45886af)), closes [#1264](https://github.com/patchloom/patchloom/issues/1264) [#1265](https://github.com/patchloom/patchloom/issues/1265)
* **ast:** multi-language rewrite_function_signature ([#1271](https://github.com/patchloom/patchloom/issues/1271)) ([9c179de](https://github.com/patchloom/patchloom/commit/9c179de3a0e9828f3132df1606cfd6210aecdfd5))
* **mcp:** add tool category guide to server instructions ([#1275](https://github.com/patchloom/patchloom/issues/1275)) ([46a8f7e](https://github.com/patchloom/patchloom/commit/46a8f7e62530dd69f21a93d4c857ad9494b52fab)), closes [#1273](https://github.com/patchloom/patchloom/issues/1273)
* **mcp:** improve path descriptions, add server_info, parse_unified_diff API, and fix no-match error signaling ([#1272](https://github.com/patchloom/patchloom/issues/1272)) ([6d77785](https://github.com/patchloom/patchloom/commit/6d77785cd9bd0d191de55e38ec7ffadc5cd287d7))


### Bug Fixes

* address 3 real AI code quality findings ([#1278](https://github.com/patchloom/patchloom/issues/1278)) ([9cb81cf](https://github.com/patchloom/patchloom/commit/9cb81cf41e30cd3f09c53657d5e8ee2aa22597cf))
* skip permission-based tests when running as root ([#1277](https://github.com/patchloom/patchloom/issues/1277)) ([d45671e](https://github.com/patchloom/patchloom/commit/d45671e85801ea6d7f7a5b9e3b78b1101c2420f1))
* update stale counts and add missing MCP integration tests ([#1262](https://github.com/patchloom/patchloom/issues/1262)) ([6cd9e22](https://github.com/patchloom/patchloom/commit/6cd9e229f26c9d682214a9f4229c1c415b310a8b))

## [0.7.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.6.0...patchloom-v0.7.0) (2026-06-30)


### ⚠ BREAKING CHANGES

* rename from/to to old/new in Replace and AstReplace, rename selector to key in doc operations, replace mode: Option<String> with regex: bool in plan Replace, change Plan.version from String to u32 with default 1, support open-ended line ranges ("8:" means line 8 to end).

### Features

* add Windows ARM64 and Linux musl release targets ([#947](https://github.com/patchloom/patchloom/issues/947)) ([8a16a5f](https://github.com/patchloom/patchloom/commit/8a16a5f7d5c3ff4892fc81176e8714060a1f53a4))
* align API field names with LLM priors ([#1214](https://github.com/patchloom/patchloom/issues/1214)) ([026978f](https://github.com/patchloom/patchloom/commit/026978faf05d3186e07da5d0f730bbf921418f2d))
* **ast:** add ast.insert, ast.wrap, ast.imports operations ([#1015](https://github.com/patchloom/patchloom/issues/1015), [#1017](https://github.com/patchloom/patchloom/issues/1017), [#1020](https://github.com/patchloom/patchloom/issues/1020)) ([#1039](https://github.com/patchloom/patchloom/issues/1039)) ([679b985](https://github.com/patchloom/patchloom/commit/679b98519aff16650bc85eaae571dee6b4192473)), closes [#1037](https://github.com/patchloom/patchloom/issues/1037)
* **ast:** Phase C operations and for_each glob batch ([#1040](https://github.com/patchloom/patchloom/issues/1040)) ([d0e0141](https://github.com/patchloom/patchloom/commit/d0e014123779ff47a320e34e04477af54b01fcfa))
* library API parity (FilePrepend, replace_in_content, SearchOptions) ([#1047](https://github.com/patchloom/patchloom/issues/1047)) ([973ae60](https://github.com/patchloom/patchloom/commit/973ae60850a0809893f8e6e2a70e158224f8d32e))
* post-write formatter hook with per-extension config ([#1035](https://github.com/patchloom/patchloom/issues/1035)) ([7fdc38b](https://github.com/patchloom/patchloom/commit/7fdc38bef1dbeb0238b135ede6e7bc1e63c2dd6b))
* pre/post-operation symbol verification for tx plans ([#1036](https://github.com/patchloom/patchloom/issues/1036)) ([edae25a](https://github.com/patchloom/patchloom/commit/edae25a0606d617586371f332bc0efaf76cd793e)), closes [#1019](https://github.com/patchloom/patchloom/issues/1019)
* **tidy:** add --dedent and --indent flags to tidy fix ([#1034](https://github.com/patchloom/patchloom/issues/1034)) ([2e3c9ae](https://github.com/patchloom/patchloom/commit/2e3c9ae7fca243c1aac5b11597054f8ecba1d8b4)), closes [#1021](https://github.com/patchloom/patchloom/issues/1021)
* traverse template interpolations in AST rename and refs ([#1090](https://github.com/patchloom/patchloom/issues/1090)) ([affaed9](https://github.com/patchloom/patchloom/commit/affaed9d92f746728ebd46a19be34bd5b3070dc8))


### Bug Fixes

* --exclude patterns now match relative directory paths ([#1200](https://github.com/patchloom/patchloom/issues/1200)) ([c1cf9c0](https://github.com/patchloom/patchloom/commit/c1cf9c071b29586fbcc9acf597380f77e5ac2853))
* 2 bugs found by code audit round 10 with regression tests ([#1075](https://github.com/patchloom/patchloom/issues/1075)) ([ec95536](https://github.com/patchloom/patchloom/commit/ec95536e81fd6d717754ecfa5905c91a629372af))
* 3 bugs found by code audit round 5 with regression tests ([#1070](https://github.com/patchloom/patchloom/issues/1070)) ([b2886f4](https://github.com/patchloom/patchloom/commit/b2886f48f627e007a12b4c85a9ac9ed4f06f37a4))
* 3 bugs found by code audit round 7 with regression tests ([#1072](https://github.com/patchloom/patchloom/issues/1072)) ([a9ae2f5](https://github.com/patchloom/patchloom/commit/a9ae2f5e8ddaeafc439924b471ec9c624ce42119))
* 4 bugs found by code audit round 11 with regression tests ([#1076](https://github.com/patchloom/patchloom/issues/1076)) ([7a9bdaf](https://github.com/patchloom/patchloom/commit/7a9bdafec0c278ce38d1c7434eb6464d53a006f2))
* 4 bugs found by code audit round 12 with regression tests ([#1077](https://github.com/patchloom/patchloom/issues/1077)) ([ac245e0](https://github.com/patchloom/patchloom/commit/ac245e09f16fe8c3cc37eb3633ebe784086acdb5))
* 4 bugs found by code audit round 3 with regression tests ([#1068](https://github.com/patchloom/patchloom/issues/1068)) ([fe584cd](https://github.com/patchloom/patchloom/commit/fe584cd3ca2ab8cb25ab2ce3491d4f569e3eaa82))
* 4 bugs found by code audit round 9 with regression tests ([#1074](https://github.com/patchloom/patchloom/issues/1074)) ([7ec9bf9](https://github.com/patchloom/patchloom/commit/7ec9bf9f4364d98696d39860f9ea28bca8362df3))
* 5 bugs found by code audit round 2 with regression tests ([#1067](https://github.com/patchloom/patchloom/issues/1067)) ([30390ad](https://github.com/patchloom/patchloom/commit/30390ad5171f2a8e186bffdba98660ddcf44ecc4))
* 5 bugs found by code audit round 4 with regression tests ([#1069](https://github.com/patchloom/patchloom/issues/1069)) ([86f08cc](https://github.com/patchloom/patchloom/commit/86f08cc4c39daed14d0d54d97beaac2a7e50153a))
* 5 bugs found by code audit round 6 with regression tests ([#1071](https://github.com/patchloom/patchloom/issues/1071)) ([bf1aeb7](https://github.com/patchloom/patchloom/commit/bf1aeb7d11a7d0ec3a6f6f64255eeb7946a157ca))
* 5 bugs found by code audit round 8 with regression tests ([#1073](https://github.com/patchloom/patchloom/issues/1073)) ([1b92522](https://github.com/patchloom/patchloom/commit/1b92522cb187569f1922b6aa427a3b153f9f921f))
* 5 bugs found by code audit with regression tests ([#1066](https://github.com/patchloom/patchloom/issues/1066)) ([5132560](https://github.com/patchloom/patchloom/commit/513256083ada30935526b016df49473054036dc2)), closes [#1067](https://github.com/patchloom/patchloom/issues/1067) [#1068](https://github.com/patchloom/patchloom/issues/1068) [#1069](https://github.com/patchloom/patchloom/issues/1069) [#1070](https://github.com/patchloom/patchloom/issues/1070)
* 5 bugs found by LLM audit round 4 ([#1156](https://github.com/patchloom/patchloom/issues/1156)) ([d4e9b1d](https://github.com/patchloom/patchloom/commit/d4e9b1d834798415d8ab7a74a1d57f604e3dedee)), closes [#1151](https://github.com/patchloom/patchloom/issues/1151) [#1152](https://github.com/patchloom/patchloom/issues/1152) [#1153](https://github.com/patchloom/patchloom/issues/1153) [#1154](https://github.com/patchloom/patchloom/issues/1154) [#1155](https://github.com/patchloom/patchloom/issues/1155)
* 5 bugs found by LLM audit round 5 ([#1162](https://github.com/patchloom/patchloom/issues/1162)) ([ca72b4b](https://github.com/patchloom/patchloom/commit/ca72b4b9bcdccd6c9633538b05fe45d79d296396)), closes [#1157](https://github.com/patchloom/patchloom/issues/1157) [#1158](https://github.com/patchloom/patchloom/issues/1158) [#1159](https://github.com/patchloom/patchloom/issues/1159) [#1160](https://github.com/patchloom/patchloom/issues/1160) [#1161](https://github.com/patchloom/patchloom/issues/1161)
* 5 bugs from LLM audit round 10 ([#1192](https://github.com/patchloom/patchloom/issues/1192)) ([045538b](https://github.com/patchloom/patchloom/commit/045538b71ebdae184801e324e57a645223760721)), closes [#1187](https://github.com/patchloom/patchloom/issues/1187) [#1188](https://github.com/patchloom/patchloom/issues/1188) [#1189](https://github.com/patchloom/patchloom/issues/1189) [#1190](https://github.com/patchloom/patchloom/issues/1190) [#1191](https://github.com/patchloom/patchloom/issues/1191)
* 5 bugs from LLM audit round 10 ([#1193](https://github.com/patchloom/patchloom/issues/1193)) ([5c47abf](https://github.com/patchloom/patchloom/commit/5c47abfde715a14fafca06801bb3f27b3ee5e378)), closes [#1187](https://github.com/patchloom/patchloom/issues/1187) [#1188](https://github.com/patchloom/patchloom/issues/1188) [#1189](https://github.com/patchloom/patchloom/issues/1189) [#1190](https://github.com/patchloom/patchloom/issues/1190) [#1191](https://github.com/patchloom/patchloom/issues/1191)
* 5 bugs from LLM audit round 6 ([#1168](https://github.com/patchloom/patchloom/issues/1168)) ([4cf275b](https://github.com/patchloom/patchloom/commit/4cf275baf5796fea8d03adfb8ee15a2a5f2c9203)), closes [#1163](https://github.com/patchloom/patchloom/issues/1163) [#1164](https://github.com/patchloom/patchloom/issues/1164) [#1165](https://github.com/patchloom/patchloom/issues/1165) [#1166](https://github.com/patchloom/patchloom/issues/1166) [#1167](https://github.com/patchloom/patchloom/issues/1167)
* 5 bugs from LLM audit round 7 ([#1174](https://github.com/patchloom/patchloom/issues/1174)) ([e4acc6e](https://github.com/patchloom/patchloom/commit/e4acc6ec6fede67639ecbb29b05b2a2b4d1327ad)), closes [#1169](https://github.com/patchloom/patchloom/issues/1169) [#1170](https://github.com/patchloom/patchloom/issues/1170) [#1171](https://github.com/patchloom/patchloom/issues/1171) [#1172](https://github.com/patchloom/patchloom/issues/1172) [#1173](https://github.com/patchloom/patchloom/issues/1173)
* 5 bugs from LLM audit round 8 ([#1180](https://github.com/patchloom/patchloom/issues/1180)) ([e841252](https://github.com/patchloom/patchloom/commit/e8412528691acab8318164b916a07de4dd3f82b1)), closes [#1175](https://github.com/patchloom/patchloom/issues/1175) [#1176](https://github.com/patchloom/patchloom/issues/1176) [#1177](https://github.com/patchloom/patchloom/issues/1177) [#1178](https://github.com/patchloom/patchloom/issues/1178) [#1179](https://github.com/patchloom/patchloom/issues/1179)
* 5 bugs from LLM audit round 9 ([#1186](https://github.com/patchloom/patchloom/issues/1186)) ([b6fe1bf](https://github.com/patchloom/patchloom/commit/b6fe1bf841515acba3de8592960a47e09dad63a2)), closes [#1181](https://github.com/patchloom/patchloom/issues/1181) [#1182](https://github.com/patchloom/patchloom/issues/1182) [#1183](https://github.com/patchloom/patchloom/issues/1183) [#1184](https://github.com/patchloom/patchloom/issues/1184) [#1185](https://github.com/patchloom/patchloom/issues/1185)
* 5 LLM-audit round 2 bugs across AST, MCP, and plan engine ([#1144](https://github.com/patchloom/patchloom/issues/1144)) ([6bce5b8](https://github.com/patchloom/patchloom/commit/6bce5b8683e8ca8d3f520f64f8ca93f6c9fedb76)), closes [#1139](https://github.com/patchloom/patchloom/issues/1139) [#1140](https://github.com/patchloom/patchloom/issues/1140) [#1141](https://github.com/patchloom/patchloom/issues/1141) [#1142](https://github.com/patchloom/patchloom/issues/1142) [#1143](https://github.com/patchloom/patchloom/issues/1143)
* 5 LLM-audit round 3 bugs across write, config, rename, schema, verify ([#1150](https://github.com/patchloom/patchloom/issues/1150)) ([e8e7315](https://github.com/patchloom/patchloom/commit/e8e7315fe961b195a153460c038b4ed76fb2373b)), closes [#1145](https://github.com/patchloom/patchloom/issues/1145) [#1146](https://github.com/patchloom/patchloom/issues/1146) [#1147](https://github.com/patchloom/patchloom/issues/1147) [#1148](https://github.com/patchloom/patchloom/issues/1148) [#1149](https://github.com/patchloom/patchloom/issues/1149)
* 9 bugs found by code review with regression tests ([#1064](https://github.com/patchloom/patchloom/issues/1064)) ([b78329b](https://github.com/patchloom/patchloom/commit/b78329b6435ef844f8fdb7fa9200fe2d07d77bb5))
* 9 LLM-audit bugs with tests across imports, markdown, tx, and CLI ([#1138](https://github.com/patchloom/patchloom/issues/1138)) ([8c41ba6](https://github.com/patchloom/patchloom/commit/8c41ba6bf96961dfe5c117db67565f316c650005)), closes [#1129](https://github.com/patchloom/patchloom/issues/1129) [#1130](https://github.com/patchloom/patchloom/issues/1130) [#1131](https://github.com/patchloom/patchloom/issues/1131) [#1132](https://github.com/patchloom/patchloom/issues/1132) [#1133](https://github.com/patchloom/patchloom/issues/1133) [#1134](https://github.com/patchloom/patchloom/issues/1134) [#1135](https://github.com/patchloom/patchloom/issues/1135) [#1136](https://github.com/patchloom/patchloom/issues/1136) [#1137](https://github.com/patchloom/patchloom/issues/1137)
* add dedicated Ruby AST symbol extractor ([#1241](https://github.com/patchloom/patchloom/issues/1241)) ([5017bad](https://github.com/patchloom/patchloom/commit/5017badb1d9d0c11878fbbaffcb0c79e6b3e79a4))
* add verbose diagnostic coverage across all commands ([#1118](https://github.com/patchloom/patchloom/issues/1118)) ([62332cf](https://github.com/patchloom/patchloom/commit/62332cf4f3249c9e34a06ad9f0f6715d06c96859)), closes [#1117](https://github.com/patchloom/patchloom/issues/1117)
* address AI code quality findings across 4 files ([#1218](https://github.com/patchloom/patchloom/issues/1218)) ([93f0af7](https://github.com/patchloom/patchloom/commit/93f0af739ef5cdd52405a3ba2a156db6e8f38ba0))
* address issues [#1101](https://github.com/patchloom/patchloom/issues/1101), [#1107](https://github.com/patchloom/patchloom/issues/1107), [#1108](https://github.com/patchloom/patchloom/issues/1108), [#1111](https://github.com/patchloom/patchloom/issues/1111) (items 2, 3, 6) ([#1113](https://github.com/patchloom/patchloom/issues/1113)) ([4580802](https://github.com/patchloom/patchloom/commit/45808029826e02f86abd9f8f539a02a23a746c90))
* assert_count ok field, add_imports placement, explain display, batch grammar ([#1094](https://github.com/patchloom/patchloom/issues/1094)) ([bdf6be0](https://github.com/patchloom/patchloom/commit/bdf6be06a47b625942054d50b977e1fd2f2a1a94))
* ast groups Go receiver methods under their receiver type ([#1206](https://github.com/patchloom/patchloom/issues/1206)) ([f4e33cf](https://github.com/patchloom/patchloom/commit/f4e33cffae1d316b2a3ffc08a6d17e64c06c6eaf))
* ast read qualified name tries all matching parents ([#1205](https://github.com/patchloom/patchloom/issues/1205)) ([81036ea](https://github.com/patchloom/patchloom/commit/81036eac58bd3772dfe61c002479559d15babc4e))
* ast search pattern mode silently fails on meta-variables ([#1240](https://github.com/patchloom/patchloom/issues/1240)) ([3709a2a](https://github.com/patchloom/patchloom/commit/3709a2a4bc901cb10c1902bb4c2fdcf8001a47fb))
* ATX heading CommonMark compliance and reorder slice panic ([#1084](https://github.com/patchloom/patchloom/issues/1084)) ([c2077a8](https://github.com/patchloom/patchloom/commit/c2077a818b3f20339cdc106929c5c7f68b524ab9))
* C++ qualified name handling in AST operations and refactor traversal ([#1055](https://github.com/patchloom/patchloom/issues/1055)) ([bddf621](https://github.com/patchloom/patchloom/commit/bddf62151ce9eecbc13d5e71d692315fd9d3055d))
* C++ qualified name lookup and empty append spurious newline ([#1054](https://github.com/patchloom/patchloom/issues/1054)) ([dd45ec8](https://github.com/patchloom/patchloom/commit/dd45ec8736bf28da70a67a2f3ddd324d50b3d032)), closes [#1052](https://github.com/patchloom/patchloom/issues/1052) [#1053](https://github.com/patchloom/patchloom/issues/1053)
* clean trailing whitespace from YAML CST after key deletion ([#1237](https://github.com/patchloom/patchloom/issues/1237)) ([4311554](https://github.com/patchloom/patchloom/commit/43115546f7f71e8d859f000a9b60145227de50c0))
* close verbose coverage gaps in read, doc, explain, init, schema ([#1120](https://github.com/patchloom/patchloom/issues/1120)) ([9cc8b22](https://github.com/patchloom/patchloom/commit/9cc8b229a4bf78d98552e57ebd03c573c8a91e11)), closes [#1119](https://github.com/patchloom/patchloom/issues/1119)
* config color=auto spurious warning, ast search global max-results ([#1093](https://github.com/patchloom/patchloom/issues/1093)) ([2c7c976](https://github.com/patchloom/patchloom/commit/2c7c9761b26f8d702fd8e5226389192155b82404))
* context-filtered replace, multi-doc YAML, nested predicates, simple-array delete-where ([#1248](https://github.com/patchloom/patchloom/issues/1248)) ([1dfc4ea](https://github.com/patchloom/patchloom/commit/1dfc4ea14283c4e72e9715f9ef10ca6e8ac868d3)), closes [#1244](https://github.com/patchloom/patchloom/issues/1244) [#1245](https://github.com/patchloom/patchloom/issues/1245) [#1246](https://github.com/patchloom/patchloom/issues/1246) [#1247](https://github.com/patchloom/patchloom/issues/1247)
* convention violations and stale doc comments from [#1208](https://github.com/patchloom/patchloom/issues/1208) rename ([#1219](https://github.com/patchloom/patchloom/issues/1219)) ([d8f9613](https://github.com/patchloom/patchloom/commit/d8f96138e66b3b8ea28d3467ab579feb6ece65c0))
* create with empty content now actually creates the file ([#1233](https://github.com/patchloom/patchloom/issues/1233)) ([4501590](https://github.com/patchloom/patchloom/commit/4501590f46d2b3ae137871d32ce37a404dccd02b))
* dead code and weak test assertions ([#1058](https://github.com/patchloom/patchloom/issues/1058)) ([d1e19c5](https://github.com/patchloom/patchloom/commit/d1e19c594217cd69fe75f2b0fcbc51c9987227db))
* doc operations no longer reformat unchanged JSON files ([#1204](https://github.com/patchloom/patchloom/issues/1204)) ([3f1c478](https://github.com/patchloom/patchloom/commit/3f1c478d3ba01fa0c4b2086179abc2e2a149a342))
* enable multi_line(true) on all regex builders for consistent anchor behavior ([#1254](https://github.com/patchloom/patchloom/issues/1254)) ([fe0868a](https://github.com/patchloom/patchloom/commit/fe0868afc4f955cfd8838672d5934f2a4e63aa4a))
* ensure_final_newline respects EOL mode ([#994](https://github.com/patchloom/patchloom/issues/994)) ([#995](https://github.com/patchloom/patchloom/issues/995)) ([5a9cdbc](https://github.com/patchloom/patchloom/commit/5a9cdbcc767b38a691f16acf89652a63e7d3eb75))
* extract C/C++ pointer-returning functions in AST listing ([#1242](https://github.com/patchloom/patchloom/issues/1242)) ([93bf306](https://github.com/patchloom/patchloom/commit/93bf30638d3c8ede645239110082c5a03532e94a))
* for_each escape mechanism, AST span overlap detection ([#1114](https://github.com/patchloom/patchloom/issues/1114)) ([f72fb60](https://github.com/patchloom/patchloom/commit/f72fb6076bc6d579f859c6e7d8967dd79f4bc62a))
* four bugs in refs, selector parser, fallback, and backup ([#1081](https://github.com/patchloom/patchloom/issues/1081)) ([5ba43b3](https://github.com/patchloom/patchloom/commit/5ba43b3223f5dbe7e0cc1e0194260d4f465997bd))
* handle CR and CRLF line endings in replace_whole_lines ([#1000](https://github.com/patchloom/patchloom/issues/1000)) ([098c1bf](https://github.com/patchloom/patchloom/commit/098c1bfd9bd39c7ed9e13c72317ccb2b93287e27)), closes [#999](https://github.com/patchloom/patchloom/issues/999)
* handle CR-only line endings in collapse_blanks and trim_trailing_whitespace ([#998](https://github.com/patchloom/patchloom/issues/998)) ([2c43864](https://github.com/patchloom/patchloom/commit/2c43864b35398b736cf188f9141a776d35cc0dd7)), closes [#996](https://github.com/patchloom/patchloom/issues/996)
* hunk context anchors, patch CRLF preservation, TOML null warning ([#1087](https://github.com/patchloom/patchloom/issues/1087)) ([31c5f94](https://github.com/patchloom/patchloom/commit/31c5f94b5070d5d881a745d767051aac5b0fe0df))
* impl trait naming, deletion patch path, Go grouped types ([#1085](https://github.com/patchloom/patchloom/issues/1085)) ([804aa09](https://github.com/patchloom/patchloom/commit/804aa090f04953458f5cdb73237bdf76ed01a270))
* improve multi-doc YAML detection, multi-line context matching, and test coverage ([#1252](https://github.com/patchloom/patchloom/issues/1252)) ([46f736f](https://github.com/patchloom/patchloom/commit/46f736fac2a72094749e8606f8ab07e4a2362027))
* include `word` node kind in AST rename for shell/bash ([#1243](https://github.com/patchloom/patchloom/issues/1243)) ([c0a8322](https://github.com/patchloom/patchloom/commit/c0a83226696cd0f82f4c74050c87e9f8a3cbae30))
* insert_inside single-line containers, unwrap_module brace-line content, backup restore error propagation ([#1098](https://github.com/patchloom/patchloom/issues/1098)) ([52a7b4b](https://github.com/patchloom/patchloom/commit/52a7b4b475820c7aae0df96a770957ed0773927f))
* Java static import path, Python from-import path, YAML key scope escape ([#1086](https://github.com/patchloom/patchloom/issues/1086)) ([23451e1](https://github.com/patchloom/patchloom/commit/23451e1c402253d851fd6c6454f099d41c425798))
* MCP description key/selector, rename backup order, line range end=0, path validation ([#1226](https://github.com/patchloom/patchloom/issues/1226)) ([eb95641](https://github.com/patchloom/patchloom/commit/eb9564105a440a008b6c67be223532b60a5c910c))
* MCP empty pattern, reverse deps precision, rollback logging, backup warning, Windows shell escape ([#1224](https://github.com/patchloom/patchloom/issues/1224)) ([189621a](https://github.com/patchloom/patchloom/commit/189621a311294c9b9deb2c05cec92928b5216460))
* MCP replace_text false warnings with case_insensitive, add md_dedupe_headings tool ([#1097](https://github.com/patchloom/patchloom/issues/1097)) ([bd24512](https://github.com/patchloom/patchloom/commit/bd2451249919f3d2dc5815f185c299f4ef2061e5))
* md --check JSON now reports actual has_changes value ([#1202](https://github.com/patchloom/patchloom/issues/1202)) ([7ae8854](https://github.com/patchloom/patchloom/commit/7ae885404ad92fd959ca347827e9fea0b215dab9))
* merge-check exit code masked by --allow-conflicts, find_match_global panic ([#1092](https://github.com/patchloom/patchloom/issues/1092)) ([74b9f2c](https://github.com/patchloom/patchloom/commit/74b9f2ca24b803c67c50f855d33da7285e6374da))
* multi-perspective improvement - tests, docs, and error context ([#956](https://github.com/patchloom/patchloom/issues/956)) ([a77afde](https://github.com/patchloom/patchloom/commit/a77afdeb10c012905ea81fe56054809d73ab94f3))
* node_signature destructuring brace, flatten_value dot-key quoting ([#1096](https://github.com/patchloom/patchloom/issues/1096)) ([ba45fd3](https://github.com/patchloom/patchloom/commit/ba45fd3616a5f1ff1041d654ce8b816374d02482))
* patch check reports stale files in human-readable mode, suppress false would-modify ([#1207](https://github.com/patchloom/patchloom/issues/1207)) ([a827511](https://github.com/patchloom/patchloom/commit/a8275117cccca022da090b626a3184f5bf4a7d33))
* patch merge exit code, dedent tabs, confirm double-prompt ([#1089](https://github.com/patchloom/patchloom/issues/1089)) ([9e19306](https://github.com/patchloom/patchloom/commit/9e1930646127abac07e17514a17e0f60e70a6f6e))
* Phase C AST ops improvements from multi-perspective review ([#1046](https://github.com/patchloom/patchloom/issues/1046)) ([465ef09](https://github.com/patchloom/patchloom/commit/465ef092be939e980e6a5fa75147596e6bb5052a))
* preserve YAML quote styles and key order on doc delete ([#1239](https://github.com/patchloom/patchloom/issues/1239)) ([9f3eb70](https://github.com/patchloom/patchloom/commit/9f3eb70def7c43bba6342c51917db86d548f0412))
* remove dead code and misleading #[allow(dead_code)] in tx engine ([#1060](https://github.com/patchloom/patchloom/issues/1060)) ([c675782](https://github.com/patchloom/patchloom/commit/c675782e4a174d0fc7ae4cf8869bd40ecdb6f738)), closes [#1059](https://github.com/patchloom/patchloom/issues/1059)
* replace --if-exists --json, ast refs const, doc move intra-array ([#1197](https://github.com/patchloom/patchloom/issues/1197)) ([dbd97eb](https://github.com/patchloom/patchloom/commit/dbd97eb6e0be247194b129bc7ab6610aee15f4b5))
* replace unreachable!() with bail!() and propagate schema errors ([#1222](https://github.com/patchloom/patchloom/issues/1222)) ([626c2b5](https://github.com/patchloom/patchloom/commit/626c2b510bfb6b4acc0b66f35be09ef19b2e49fe))
* resolve 5 issues (is_file guard, multiline search, prepend newline, split duplicate, find_function_span) ([#1051](https://github.com/patchloom/patchloom/issues/1051)) ([732765b](https://github.com/patchloom/patchloom/commit/732765b76f50befa2bad829bf195dc8152fda9ba))
* resolve symlinks in atomic_write, surface specific table_append errors ([#1232](https://github.com/patchloom/patchloom/issues/1232)) ([7f41306](https://github.com/patchloom/patchloom/commit/7f413063124c3c6b55aee8c8a1ddbfa2c52926ed))
* resolve tech-debt issues [#985](https://github.com/patchloom/patchloom/issues/985)-[#989](https://github.com/patchloom/patchloom/issues/989) ([#990](https://github.com/patchloom/patchloom/issues/990)) ([60630ed](https://github.com/patchloom/patchloom/commit/60630ed7d4ad85a91dffc57d0b0de14d0c115bc1)), closes [#986](https://github.com/patchloom/patchloom/issues/986) [#987](https://github.com/patchloom/patchloom/issues/987) [#988](https://github.com/patchloom/patchloom/issues/988)
* retry TLS connection in HTTPS MCP test to handle startup race ([#1259](https://github.com/patchloom/patchloom/issues/1259)) ([70df28f](https://github.com/patchloom/patchloom/commit/70df28f02ff4e752ca71057bfd1d9a12a1b92f3b)), closes [#1258](https://github.com/patchloom/patchloom/issues/1258)
* sequential undo now advances through backup sessions ([#1201](https://github.com/patchloom/patchloom/issues/1201)) ([465ea80](https://github.com/patchloom/patchloom/commit/465ea807f218794694f473f2afdcb5784ab75b3c))
* setext heading body offset and CRLF bare CR normalization ([#993](https://github.com/patchloom/patchloom/issues/993)) ([478804d](https://github.com/patchloom/patchloom/commit/478804d7387ee3bbf4305802b2c26c521b3900dc)), closes [#991](https://github.com/patchloom/patchloom/issues/991) [#992](https://github.com/patchloom/patchloom/issues/992)
* strengthen weak assertion and replace panic with Result ([#1128](https://github.com/patchloom/patchloom/issues/1128)) ([cf5bdb6](https://github.com/patchloom/patchloom/commit/cf5bdb6db1794367600c741a64eac21bcc04f1d0)), closes [#1126](https://github.com/patchloom/patchloom/issues/1126) [#1127](https://github.com/patchloom/patchloom/issues/1127)
* strict rollback restores collateral files from format steps ([#1116](https://github.com/patchloom/patchloom/issues/1116)) ([5d28c15](https://github.com/patchloom/patchloom/commit/5d28c15a70d2188b2f77933b083a7eb2341377af))
* strip orphaned YAML comments that migrate inline after key deletion ([#1238](https://github.com/patchloom/patchloom/issues/1238)) ([4e1e8ce](https://github.com/patchloom/patchloom/commit/4e1e8ce88d89ed80ccd458295103c799e57a8d3d))
* support file creation and deletion in patch apply ([#1235](https://github.com/patchloom/patchloom/issues/1235)) ([0e92d8c](https://github.com/patchloom/patchloom/commit/0e92d8c086d6bcbd7a86e2aa886a5681531d2210))
* suppress misleading 'modified' message when apply has no changes ([#1234](https://github.com/patchloom/patchloom/issues/1234)) ([9ddd939](https://github.com/patchloom/patchloom/commit/9ddd939d4ce92e2a3d79e9cff044eb8098f414ac))
* systematic CRLF preservation, frontmatter parsing, decorator spans, multi-declarator const, YAML comments, replace count isolation ([#1112](https://github.com/patchloom/patchloom/issues/1112)) ([c95a564](https://github.com/patchloom/patchloom/commit/c95a564e946d87f117750b509767310b3841e0f0))
* table_append error messages and ast map file grouping ([#1229](https://github.com/patchloom/patchloom/issues/1229)) ([1dd6c8e](https://github.com/patchloom/patchloom/commit/1dd6c8e56319aae25f297fa1fa2100900ba7dd84))
* three bugs in insert, config defaults, and status separators ([#1080](https://github.com/patchloom/patchloom/issues/1080)) ([40b901b](https://github.com/patchloom/patchloom/commit/40b901b24c28f49a737cab3271d3bb6fe9f8d1ae))
* three bugs in markdown ops and schema type rendering ([#1082](https://github.com/patchloom/patchloom/issues/1082)) ([c7b75a0](https://github.com/patchloom/patchloom/commit/c7b75a02b3ea72770fa9e76de9d9ce61694b981c))
* three bugs in replace, reorder, and rewrite_function_signature ([#1078](https://github.com/patchloom/patchloom/issues/1078)) ([58210ca](https://github.com/patchloom/patchloom/commit/58210cae623f6c52ba6c0e5a1bf2359925b64e45))
* three bugs in symbol spans, inline code stripping, and line ending handling ([#1079](https://github.com/patchloom/patchloom/issues/1079)) ([e85c1ee](https://github.com/patchloom/patchloom/commit/e85c1ee7e09142865045cdc2bcdd13fb5519f418))
* tidy check --respect-editorconfig now detects EOL mismatches ([#1199](https://github.com/patchloom/patchloom/issues/1199)) ([80a739d](https://github.com/patchloom/patchloom/commit/80a739d3d4434d0da679c4508df0b3d386598f2f))
* tidy check --respect-editorconfig now honors trim_trailing_whitespace ([#1203](https://github.com/patchloom/patchloom/issues/1203)) ([641a035](https://github.com/patchloom/patchloom/commit/641a0353fee335acd9f27e3ed4df810e6693fed5))
* tidy check detects EOL mismatches, md upsert-bullet accepts dash-prefixed values ([#1198](https://github.com/patchloom/patchloom/issues/1198)) ([5028e90](https://github.com/patchloom/patchloom/commit/5028e90ed1744ed1ecee0a941821cc80fbef3be3))
* tighten error matching, explain flags, and MCP doc examples ([#1225](https://github.com/patchloom/patchloom/issues/1225)) ([b797bd7](https://github.com/patchloom/patchloom/commit/b797bd7d748daf518ed053e42549e2810550e01e))
* update stale API field names in bench script and README ([#1217](https://github.com/patchloom/patchloom/issues/1217)) ([cd574f2](https://github.com/patchloom/patchloom/commit/cd574f2aa3f9d294215dfddb852597a1be76c04c)), closes [#1215](https://github.com/patchloom/patchloom/issues/1215) [#1216](https://github.com/patchloom/patchloom/issues/1216)
* upsert_bullet trailing space trim, tidy empty file, git add ./, doc merge conflict ([#1095](https://github.com/patchloom/patchloom/issues/1095)) ([51cc510](https://github.com/patchloom/patchloom/commit/51cc51091a59cb43651d7f958e859ad34559a1fd))
* use correct label for non-required validation step failures ([#1236](https://github.com/patchloom/patchloom/issues/1236)) ([ceef67f](https://github.com/patchloom/patchloom/commit/ceef67f9743d051da38d844175adbb61ac934fb1))
* validate whole-line+insert conflict, MCP context size, multiline attrs ([#1091](https://github.com/patchloom/patchloom/issues/1091)) ([ffc6b3e](https://github.com/patchloom/patchloom/commit/ffc6b3e2cac633fbadc0a51373e0780f80321deb))
* validate_edit nth, atomic_write symlink perms, rollback create-then-delete ([#1065](https://github.com/patchloom/patchloom/issues/1065)) ([e854b7c](https://github.com/patchloom/patchloom/commit/e854b7c5109eb8e5c80d838928e5e941e5d7315f))
* wrap.rs missing full_symbol_span, extract.rs brace detection, imports.rs scope awareness ([#1083](https://github.com/patchloom/patchloom/issues/1083)) ([6cb2dbe](https://github.com/patchloom/patchloom/commit/6cb2dbe0d179cb2be22eb661d799d96c73a30ec9))


### Performance Improvements

* build reverse dep map upfront in ast_impact; fix CONTRIBUTING.md ([#949](https://github.com/patchloom/patchloom/issues/949)) ([9ffd1bb](https://github.com/patchloom/patchloom/commit/9ffd1bb97a282ce423371a94ff792aa2d9a9b55d))


### Reverts

* remove speculative path validation and patch guard ([#1227](https://github.com/patchloom/patchloom/issues/1227)) ([9806567](https://github.com/patchloom/patchloom/commit/9806567b722ea31e94b49278dea1cb2e8c8b9ce7))

## [0.6.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.5.0...patchloom-v0.6.0) (2026-06-25)


### Features

* expose AST via MCP, broaden test coverage, add config/EditorConfig/fuzz improvements ([#924](https://github.com/patchloom/patchloom/issues/924)) ([58ae131](https://github.com/patchloom/patchloom/commit/58ae131a3b092cdbd66e48108d8302a03f0a7ccf))


### Bug Fixes

* add concurrent-write warnings to MCP tool descriptions ([#908](https://github.com/patchloom/patchloom/issues/908)) ([1546eff](https://github.com/patchloom/patchloom/commit/1546eff4278b9824cfa77d08c289be599958f7e0)), closes [#892](https://github.com/patchloom/patchloom/issues/892)
* address 5 structural audit findings ([#887](https://github.com/patchloom/patchloom/issues/887)) ([17e7fd0](https://github.com/patchloom/patchloom/commit/17e7fd01d6e69baaf6a379aa5a8bec44547f1812))
* AST improvements ([#927](https://github.com/patchloom/patchloom/issues/927)-[#931](https://github.com/patchloom/patchloom/issues/931)) ([#932](https://github.com/patchloom/patchloom/issues/932)) ([9b49126](https://github.com/patchloom/patchloom/commit/9b49126e27557ca4ca9c654899897145ce6a1f46)), closes [#928](https://github.com/patchloom/patchloom/issues/928) [#929](https://github.com/patchloom/patchloom/issues/929) [#930](https://github.com/patchloom/patchloom/issues/930)
* AST MCP test coverage, docs, param validation, and git arg guard ([#925](https://github.com/patchloom/patchloom/issues/925)) ([513808b](https://github.com/patchloom/patchloom/commit/513808bd6c57d198e48c36b388ec82e5cfe230b9))
* ast WritePolicy + confirm support, replace JSONL no-match ([#913](https://github.com/patchloom/patchloom/issues/913)) ([1bc2e20](https://github.com/patchloom/patchloom/commit/1bc2e208ae8a373e809fd2f09fe3c356fce1d4e8))
* exit_code_to_result uses fallback for all exit codes; rename stale test ([#941](https://github.com/patchloom/patchloom/issues/941)) ([1e54c90](https://github.com/patchloom/patchloom/commit/1e54c90284c197e95b30b91bcf45e5891ac770e1)), closes [#939](https://github.com/patchloom/patchloom/issues/939) [#940](https://github.com/patchloom/patchloom/issues/940)
* flatten includes empty arrays and empty objects ([#897](https://github.com/patchloom/patchloom/issues/897)) ([be2093a](https://github.com/patchloom/patchloom/commit/be2093a2f178c042d723dfc594a9ebb7eb873c5c)), closes [#894](https://github.com/patchloom/patchloom/issues/894)
* git arg guard, ast_rename no-op guard, 6 new AST MCP tests, ref docs ([#926](https://github.com/patchloom/patchloom/issues/926)) ([cdd9f98](https://github.com/patchloom/patchloom/commit/cdd9f98b638ece648aa089689b3ede30c05ca5c9))
* make release notes cleanup non-fatal (continue-on-error) ([#874](https://github.com/patchloom/patchloom/issues/874)) ([a18d65e](https://github.com/patchloom/patchloom/commit/a18d65e25c833c1339db4bc1e71be380626bdac4))
* preserve idempotent delete semantics in tx engine ([#889](https://github.com/patchloom/patchloom/issues/889)) ([39a5b87](https://github.com/patchloom/patchloom/commit/39a5b875444c0eb9e8c055a8b83bdd0ce17a944c))
* remove release PR body editor that broke v0.5.0 release ([#873](https://github.com/patchloom/patchloom/issues/873)) ([6749e89](https://github.com/patchloom/patchloom/commit/6749e8904fd708afd97d0972f8b7567ba08317e4))
* tx validation gaps, search --jsonl no-match, YAML MCP tests ([#907](https://github.com/patchloom/patchloom/issues/907)) ([327f2a6](https://github.com/patchloom/patchloom/commit/327f2a6fa4b89cdddca4ecf40b9d7a160d9baac4))
* validate delete_where predicates and extract DocMutation dispatch ([#888](https://github.com/patchloom/patchloom/issues/888)) ([b2b5d00](https://github.com/patchloom/patchloom/commit/b2b5d00c05d9a6cf413c89d93ff2574e7d5f4868))


### Performance Improvements

* MCP parallelism, tree cache, spawn_blocking; test: PageRank + unsupported lang ([#938](https://github.com/patchloom/patchloom/issues/938)) ([fa6fa29](https://github.com/patchloom/patchloom/commit/fa6fa293f77a6d612dde34c5a523705047a5ca71))

## [0.5.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.4.0...patchloom-v0.5.0) (2026-06-24)


### Features

* add Streamable HTTP/HTTPS transport to mcp-server ([#866](https://github.com/patchloom/patchloom/issues/866)) ([30124e3](https://github.com/patchloom/patchloom/commit/30124e3ff7ca8120882684f8876b1f8c079fa57b))
* Agent embedder [#805](https://github.com/patchloom/patchloom/issues/805) downstream polish (PlanReport, search/AST docs, versions) ([#810](https://github.com/patchloom/patchloom/issues/810)) ([be7db34](https://github.com/patchloom/patchloom/commit/be7db3413ff02b7c763b005336d5b666d83e422b))
* Library embedder follow-ups ([#811](https://github.com/patchloom/patchloom/issues/811)-[#815](https://github.com/patchloom/patchloom/issues/815)) ([#817](https://github.com/patchloom/patchloom/issues/817)) ([f421f57](https://github.com/patchloom/patchloom/commit/f421f57c95145372b7aed22aea2cc0ef3a7f685b))
* Library embedder follow-ups for execute_plan typed return, search primitives/format/ignore helper, richer types, errors, versions sync ([#811](https://github.com/patchloom/patchloom/issues/811)-[#815](https://github.com/patchloom/patchloom/issues/815)) ([#816](https://github.com/patchloom/patchloom/issues/816)) ([4dcddf4](https://github.com/patchloom/patchloom/commit/4dcddf47e96e91ccae94fe231ecbf5189a2b7539))
* full CLI/MCP/plan parity for agent search ignore layering ([#821](https://github.com/patchloom/patchloom/issues/821)) ([#822](https://github.com/patchloom/patchloom/issues/822)) ([f7c0a8f](https://github.com/patchloom/patchloom/commit/f7c0a8fde8c2f04591ec0c22eb36374380e4f043))
* library embedding gaps for pure 'ast'+'files' ([#792](https://github.com/patchloom/patchloom/issues/792)) ([#793](https://github.com/patchloom/patchloom/issues/793)) ([f0f99ef](https://github.com/patchloom/patchloom/commit/f0f99ef2b137633463312aad4be782868f2fda6e))
* **lib:** ungate files helpers + add directory search API ([#773](https://github.com/patchloom/patchloom/issues/773) [#774](https://github.com/patchloom/patchloom/issues/774)) ([#775](https://github.com/patchloom/patchloom/issues/775)) ([505240c](https://github.com/patchloom/patchloom/commit/505240c6d48894e937ad775863147d12345b890f))
* **mcp:** add execute_plan tool for multi-step tx plans + document cross-call semantics ([#827](https://github.com/patchloom/patchloom/issues/827)) ([#829](https://github.com/patchloom/patchloom/issues/829)) ([9b66e9a](https://github.com/patchloom/patchloom/commit/9b66e9abc5442b3a47001ce1e723268fc34559ba))
* search ignore customization for [#796](https://github.com/patchloom/patchloom/issues/796), AST rewrite helpers for [#797](https://github.com/patchloom/patchloom/issues/797), guard/WritePolicy audit+docs for [#801](https://github.com/patchloom/patchloom/issues/801) ([#809](https://github.com/patchloom/patchloom/issues/809)) ([4ee9bb5](https://github.com/patchloom/patchloom/commit/4ee9bb5ab1442e9c7ea9f96f4eff3e8b05cb460d))


### Bug Fixes

* align version strings to 0.4 and expose search_one_file (embedder feedback on [#817](https://github.com/patchloom/patchloom/issues/817)) ([#818](https://github.com/patchloom/patchloom/issues/818)) ([1637cb3](https://github.com/patchloom/patchloom/commit/1637cb33cdebf06cb3a5af45f495aef5299cd85d))
* **containment:** make allow_temp_directory() and allow_workspace_and_temp_dir() handle conventional /tmp paths on macOS ([#781](https://github.com/patchloom/patchloom/issues/781)) ([#782](https://github.com/patchloom/patchloom/issues/782)) ([15bdc53](https://github.com/patchloom/patchloom/commit/15bdc53c52563f1a0288fb50a6f8ac3c29c8a123))
* FOSSA false positive and TLS error path test ([#870](https://github.com/patchloom/patchloom/issues/870)) ([2c81d04](https://github.com/patchloom/patchloom/commit/2c81d04c6f09d6d478c160cc9ff2a0041f691208))
* HTTPS banner shows real port; add TLS round-trip test ([#869](https://github.com/patchloom/patchloom/issues/869)) ([df6839f](https://github.com/patchloom/patchloom/commit/df6839ffba996a2a3464498c9687d43c6f29a74d)), closes [#867](https://github.com/patchloom/patchloom/issues/867) [#868](https://github.com/patchloom/patchloom/issues/868)
* remove redundant allow(dead_code) and fix vacuous test assertions ([#865](https://github.com/patchloom/patchloom/issues/865)) ([8717eb2](https://github.com/patchloom/patchloom/commit/8717eb220653a8bf3795a309568f5e43c964eef0))
* route file MCP tools through tx engine for structured JSON responses ([#861](https://github.com/patchloom/patchloom/issues/861)) ([cd65cf6](https://github.com/patchloom/patchloom/commit/cd65cf6c14bacaf7bb04624462758901734aaf2a)), closes [#859](https://github.com/patchloom/patchloom/issues/859)
* YAML nested keys structure and md move-section bodies ([#824](https://github.com/patchloom/patchloom/issues/824), [#825](https://github.com/patchloom/patchloom/issues/825)) ([#826](https://github.com/patchloom/patchloom/issues/826)) ([26eb0fc](https://github.com/patchloom/patchloom/commit/26eb0fc33e5b4c82f82bccf8a1cfaa4691f83fad))

## [0.4.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.3.0...patchloom-v0.4.0) (2026-06-22)


### ⚠ BREAKING CHANGES

* All public structs and enums in the API surface are now marked #[non_exhaustive]. External code that constructs these types via struct literals must use ..Default::default() or equivalent patterns. Serde deserialization (the primary construction path) is unaffected.

### Features

* add --confirm flag for interactive preview-then-apply workflow ([c7c0796](https://github.com/patchloom/patchloom/commit/c7c07966fb2331e1ec1bffcf5469341af4fde040)), closes [#354](https://github.com/patchloom/patchloom/issues/354)
* add --whole-line, --range, and --collapse-blanks to replace ([#564](https://github.com/patchloom/patchloom/issues/564)) ([5651320](https://github.com/patchloom/patchloom/commit/56513207a0c7f7c18b3745825fc369eb04cc1271)), closes [#563](https://github.com/patchloom/patchloom/issues/563)
* add --word-boundary flag to prevent partial-word matches ([#657](https://github.com/patchloom/patchloom/issues/657)) ([91893e2](https://github.com/patchloom/patchloom/commit/91893e2cafc1f615904665e83cf7dabd12d6e8ba))
* add AST-aware operations using tree-sitter (20 languages) ([#658](https://github.com/patchloom/patchloom/issues/658)) ([17223e5](https://github.com/patchloom/patchloom/commit/17223e515a9e625999bc5dbff454b2ea2ec2b0df)), closes [#647](https://github.com/patchloom/patchloom/issues/647)
* add cargo-semver-checks CI and fix all rustdoc warnings ([#615](https://github.com/patchloom/patchloom/issues/615)) ([250dcb5](https://github.com/patchloom/patchloom/commit/250dcb52191b22ca3beda879715ac7000dd306a7)), closes [#612](https://github.com/patchloom/patchloom/issues/612) [#613](https://github.com/patchloom/patchloom/issues/613)
* add Claude Code and Aider agent drivers ([75a33c3](https://github.com/patchloom/patchloom/commit/75a33c3f0c6c990ac58d6c1e0111fba001ce2424))
* add Codex CLI and Cline agent drivers ([97281d7](https://github.com/patchloom/patchloom/commit/97281d72100a69c84ff44ce3b591633434a68683))
* add file.append operation and --format flag on write commands ([#667](https://github.com/patchloom/patchloom/issues/667)) ([9a57f06](https://github.com/patchloom/patchloom/commit/9a57f06d58ac979701a3bfda9b3a8c7c514cf814)), closes [#661](https://github.com/patchloom/patchloom/issues/661) [#662](https://github.com/patchloom/patchloom/issues/662)
* add inline examples to MCP tool descriptions and simplify bench prompts ([#479](https://github.com/patchloom/patchloom/issues/479)) ([fbd8336](https://github.com/patchloom/patchloom/commit/fbd83363b6a990b13034fa0929a545470693ae75))
* add MCP benchmark suite (make bench-mcp) ([#470](https://github.com/patchloom/patchloom/issues/470)) ([dc81fdd](https://github.com/patchloom/patchloom/commit/dc81fdd2d32f91e93abfa602d3a9d7bcf7206c0a))
* add patchloom explain command for human-readable plan descriptions ([f1a0056](https://github.com/patchloom/patchloom/commit/f1a00568cd19c90f7c800dbf627799a865d59b6d)), closes [#356](https://github.com/patchloom/patchloom/issues/356)
* add usage examples to --help output for all commands ([85bc1a6](https://github.com/patchloom/patchloom/commit/85bc1a6b655d76061c1b7e5750476a5c47a6ab33)), closes [#352](https://github.com/patchloom/patchloom/issues/352)
* add usage examples to MCP agent-rules output ([#471](https://github.com/patchloom/patchloom/issues/471)) ([e6eb618](https://github.com/patchloom/patchloom/commit/e6eb618a3b2b27837da3abe481344dec2a30d706))
* **api:** thread PathGuard through high-level api::* ([#749](https://github.com/patchloom/patchloom/issues/749) [#750](https://github.com/patchloom/patchloom/issues/750)) ([#752](https://github.com/patchloom/patchloom/issues/752)) ([ab5614f](https://github.com/patchloom/patchloom/commit/ab5614f6947d1eaba7faab235c573c125185fd7d))
* **ast:** add map, replace, impact, and diff subcommands ([#650](https://github.com/patchloom/patchloom/issues/650), [#653](https://github.com/patchloom/patchloom/issues/653), [#654](https://github.com/patchloom/patchloom/issues/654), [#655](https://github.com/patchloom/patchloom/issues/655)) ([#660](https://github.com/patchloom/patchloom/issues/660)) ([d4eef5a](https://github.com/patchloom/patchloom/commit/d4eef5a17db9891cabdfbd265cbabdfa2534c588))
* **ast:** add search, refs, and deps subcommands ([#649](https://github.com/patchloom/patchloom/issues/649), [#651](https://github.com/patchloom/patchloom/issues/651), [#652](https://github.com/patchloom/patchloom/issues/652)) ([#659](https://github.com/patchloom/patchloom/issues/659)) ([72eccb8](https://github.com/patchloom/patchloom/commit/72eccb83405e54ce92676c4be485ad1e0779c8b6))
* auto-install shell completions in patchloom init ([019e793](https://github.com/patchloom/patchloom/commit/019e7938ee64ac47c4fc31464cc95e69dd592bc6)), closes [#353](https://github.com/patchloom/patchloom/issues/353)
* benchmark reproducibility (README, dry-run, report, CI) ([fa96a98](https://github.com/patchloom/patchloom/commit/fa96a982973d1941b29a023325020077656a7f88)), closes [#346](https://github.com/patchloom/patchloom/issues/346)
* close [#573](https://github.com/patchloom/patchloom/issues/573) and [#574](https://github.com/patchloom/patchloom/issues/574) - complete API parity and edge case tests ([#576](https://github.com/patchloom/patchloom/issues/576)) ([d6fc1a9](https://github.com/patchloom/patchloom/commit/d6fc1a99fb1f36045bd309a4707d8f4a84919bb5))
* diff summary line after preview output ([3952718](https://github.com/patchloom/patchloom/commit/3952718285deb778fd7b3e71f4275d596d214822)), closes [#359](https://github.com/patchloom/patchloom/issues/359)
* enable MCP feature by default ([#502](https://github.com/patchloom/patchloom/issues/502)) ([7eb8750](https://github.com/patchloom/patchloom/commit/7eb87507e5b686a1d80391e50a97ce50abdd51a0))
* extract path containment into public module ([#609](https://github.com/patchloom/patchloom/issues/609)) ([d3d9ae2](https://github.com/patchloom/patchloom/commit/d3d9ae26e2b07c9867a3ac38461c637d84e6bd44))
* flexible AbsolutePathPolicy + PathGuard builder for library users ([#748](https://github.com/patchloom/patchloom/issues/748) [#749](https://github.com/patchloom/patchloom/issues/749) [#750](https://github.com/patchloom/patchloom/issues/750)) ([#751](https://github.com/patchloom/patchloom/issues/751)) ([bbab729](https://github.com/patchloom/patchloom/commit/bbab72955d83b49aed1188e9c2c6388b799d0efa)), closes [#746](https://github.com/patchloom/patchloom/issues/746)
* harden tx rollback and add three-way patch merge ([#587](https://github.com/patchloom/patchloom/issues/587)) ([db21982](https://github.com/patchloom/patchloom/commit/db2198222aa159d4b8b4874e14c4dbd399569909))
* make files module public and extract exec module ([#610](https://github.com/patchloom/patchloom/issues/610)) ([746701a](https://github.com/patchloom/patchloom/commit/746701a9030ac6dc81ce78ef1594e33bcfb8fe6f))
* mark all public types as #[non_exhaustive] for semver safety ([#624](https://github.com/patchloom/patchloom/issues/624)) ([b3592e2](https://github.com/patchloom/patchloom/commit/b3592e20ead71c62f6bc302bee432182447f0fed))
* MCP benchmark 11/11 via anti-CLI instructions and diagnostic logging ([#478](https://github.com/patchloom/patchloom/issues/478)) ([d2e776d](https://github.com/patchloom/patchloom/commit/d2e776d4d2abe4b42fb60ec464bc91976db1ac59))
* **mcp:** add batch_replace and batch_tidy homogeneous batch tools ([#486](https://github.com/patchloom/patchloom/issues/486)) ([73981b4](https://github.com/patchloom/patchloom/commit/73981b4a588766be1399f258c46692a46553e6bd))
* md.move-section -- move a heading section between files ([#554](https://github.com/patchloom/patchloom/issues/554)) ([d6f42e7](https://github.com/patchloom/patchloom/commit/d6f42e7e97db115d3506ab8295c4e261aee2f67e)), closes [#553](https://github.com/patchloom/patchloom/issues/553)
* op name aliases, consolidate doc_query, dynamic bench timeout ([#480](https://github.com/patchloom/patchloom/issues/480)) ([ffd0e3c](https://github.com/patchloom/patchloom/commit/ffd0e3c6fbf113b8ac712f32a4e3a2b0ac7a34cd))
* project config file (.patchloom.toml) for per-project defaults ([a02f71f](https://github.com/patchloom/patchloom/commit/a02f71fb1b051065803a127876d3f35098ab11ff)), closes [#355](https://github.com/patchloom/patchloom/issues/355)
* public Rust library API with thread safety, intent format, and fallback chain ([#530](https://github.com/patchloom/patchloom/issues/530)) ([093eb8b](https://github.com/patchloom/patchloom/commit/093eb8bc0abf4d567027fd9a726934943823e1e2))
* re-export EolMode from write module ([#611](https://github.com/patchloom/patchloom/issues/611)) ([fc604d9](https://github.com/patchloom/patchloom/commit/fc604d9b60963e73d1cef461c1cb1899648b0564))
* smart error recovery hints for no-match results ([fc3e7f3](https://github.com/patchloom/patchloom/commit/fc3e7f3510fd89c41c05708312621ac483a805f4)), closes [#357](https://github.com/patchloom/patchloom/issues/357)
* strengthen MCP agent-rules with tool selection guide ([#472](https://github.com/patchloom/patchloom/issues/472)) ([813d30f](https://github.com/patchloom/patchloom/commit/813d30f2862cc89371be795abe2d44eb9658a917))
* structured JSON APIs for batch and transaction MCP tools ([#473](https://github.com/patchloom/patchloom/issues/473)) ([84bed9f](https://github.com/patchloom/patchloom/commit/84bed9f89aa55c6e75c5c6143a4a9570e87b827b))
* support RELEASE_NOTES.md override for curated release descriptions ([#627](https://github.com/patchloom/patchloom/issues/627)) ([f0f92be](https://github.com/patchloom/patchloom/commit/f0f92be348c2371ad625770cd260092a077c12b8))
* tx search directory support, MCP lint-agents tool, example 08 smoke test ([6cf582b](https://github.com/patchloom/patchloom/commit/6cf582bf0fba079ecbf14b0a640d6110b8b6f32e))
* undo safety net with backup sessions ([4119e9a](https://github.com/patchloom/patchloom/commit/4119e9a02da3789aaf23f65db990b3836e98fd6a)), closes [#358](https://github.com/patchloom/patchloom/issues/358)


### Bug Fixes

* 4 Windows integration test failures ([3035e2a](https://github.com/patchloom/patchloom/commit/3035e2ae2e271c5d60a79faf3ba26f1df8feb24d))
* add AST and word_boundary to all operation surfaces ([#666](https://github.com/patchloom/patchloom/issues/666)) ([ff85bb7](https://github.com/patchloom/patchloom/commit/ff85bb756654dd2504b78242685c8b9b7658905f)), closes [#663](https://github.com/patchloom/patchloom/issues/663) [#664](https://github.com/patchloom/patchloom/issues/664) [#665](https://github.com/patchloom/patchloom/issues/665)
* add backup support to delete and rename commands ([cc8e8c2](https://github.com/patchloom/patchloom/commit/cc8e8c29827d86c37d0275b87cab685c457724cc))
* add benchmark result directories to .gitignore ([#500](https://github.com/patchloom/patchloom/issues/500)) ([9bdd383](https://github.com/patchloom/patchloom/commit/9bdd38319f7fe2713e4b22eea24fa99244a9d392))
* add error context to backup restore and rename cross-device paths ([#543](https://github.com/patchloom/patchloom/issues/543)) ([69018e7](https://github.com/patchloom/patchloom/commit/69018e784e9a5594b70000275167d15d67a1a0a0))
* add missing 'rename' to subcommand set, deduplicate driver helpers ([f0f7a37](https://github.com/patchloom/patchloom/commit/f0f7a37ca32ac87a95d6ab1bf7de3a30db34933f))
* add missing subcommands to agent driver subcommand set ([#444](https://github.com/patchloom/patchloom/issues/444)) ([4804b2d](https://github.com/patchloom/patchloom/commit/4804b2dbe1c76dd25eb8a55f52bead2927be855d))
* add test coverage and update install instructions for v0.1.0 ([#465](https://github.com/patchloom/patchloom/issues/465)) ([c42c7c1](https://github.com/patchloom/patchloom/commit/c42c7c1c2b726e95d64478f0c6656c79a5f46f18))
* add wasi crate to FOSSA false positive filter ([#510](https://github.com/patchloom/patchloom/issues/510)) ([1882060](https://github.com/patchloom/patchloom/commit/18820609dcd0fea3062e70a7e173f10836682464))
* address AI code quality findings in GrokDriver ([#453](https://github.com/patchloom/patchloom/issues/453)) ([1ecf4dc](https://github.com/patchloom/patchloom/commit/1ecf4dc79b45a1dbaa70e99df4c83bb3c0f8e1e5))
* agent bench file_ops collision and use focused agent-rules modes ([f86e02b](https://github.com/patchloom/patchloom/commit/f86e02b2ba1b3cab3eefe54e5453ce48c0432696))
* auto-sync PATCHLOOM.md on release-please version bumps ([#513](https://github.com/patchloom/patchloom/issues/513)) ([cb6cb1c](https://github.com/patchloom/patchloom/commit/cb6cb1c3dca42974c2230485d3a712dd3ac05b75)), closes [#512](https://github.com/patchloom/patchloom/issues/512)
* bench CI replace uses wrong --from flag syntax ([cedaea0](https://github.com/patchloom/patchloom/commit/cedaea0691cc478d02bc30bfa8d043266a8dcc42)), closes [#343](https://github.com/patchloom/patchloom/issues/343)
* **bench:** prefer newest binary, add per-tool MCP log reporting ([#489](https://github.com/patchloom/patchloom/issues/489)) ([3868408](https://github.com/patchloom/patchloom/commit/3868408960a3385ca3ec28709c7e05d73f26ce99))
* **bench:** use neutral tidy prompt so agents discover batch_tidy ([#490](https://github.com/patchloom/patchloom/issues/490)) ([9fb5e90](https://github.com/patchloom/patchloom/commit/9fb5e90ad8276a65710adc2b92c9d249a355372a))
* **ci:** add checkout step to release host job ([#498](https://github.com/patchloom/patchloom/issues/498)) ([3bb61d2](https://github.com/patchloom/patchloom/commit/3bb61d20e9b010a37d02eb3744cc12bda2c90ac0))
* **ci:** correct SBOM upload path for cargo-cyclonedx ([#462](https://github.com/patchloom/patchloom/issues/462)) ([07e7bd1](https://github.com/patchloom/patchloom/commit/07e7bd17054cb997f924efc43678c470d4b6149e))
* **ci:** disable fossa test until false positives are filtered ([#437](https://github.com/patchloom/patchloom/issues/437)) ([46e0e04](https://github.com/patchloom/patchloom/commit/46e0e04109f75cebd575117f2244b9169dae365e))
* **ci:** exclude securityscorecards.dev from lychee link checks ([#464](https://github.com/patchloom/patchloom/issues/464)) ([04ceac8](https://github.com/patchloom/patchloom/commit/04ceac8a98dee83ccabefc40bf879d5643c3e400))
* **ci:** make coverage badge step non-fatal when GIST_TOKEN missing ([#445](https://github.com/patchloom/patchloom/issues/445)) ([73c2e1e](https://github.com/patchloom/patchloom/commit/73c2e1e1b60ed141bbd0fff651bf0cdb93caa3de))
* **ci:** move FOSSA secret check from job-level to step-level ([#436](https://github.com/patchloom/patchloom/issues/436)) ([3f3be6f](https://github.com/patchloom/patchloom/commit/3f3be6fa1c19f35751f217877cb1cdf08b285ce4))
* **ci:** prevent transitive skip of release build jobs ([#496](https://github.com/patchloom/patchloom/issues/496)) ([fce0776](https://github.com/patchloom/patchloom/commit/fce0776bffed0a4e8649b7fc3dd979c920d9b86a))
* **ci:** remove dependabot[bot] from auto-approve actor list ([#648](https://github.com/patchloom/patchloom/issues/648)) ([677982d](https://github.com/patchloom/patchloom/commit/677982dae876dd9c3f1b2682af38d1657027381a))
* **ci:** remove duplicate release creation in cargo-dist workflow ([#492](https://github.com/patchloom/patchloom/issues/492)) ([21ece8c](https://github.com/patchloom/patchloom/commit/21ece8c2e2a541fe1bcf57cc144ab8c88fd13c9c))
* **ci:** resolve Scorecard findings for token permissions and pinned deps ([#438](https://github.com/patchloom/patchloom/issues/438)) ([877f1fe](https://github.com/patchloom/patchloom/commit/877f1fe5c46738f7eef329436dfcab8b6e5f1a39))
* **ci:** slim release manifest output to prevent silent GitHub Actions drop ([#494](https://github.com/patchloom/patchloom/issues/494)) ([88d5fda](https://github.com/patchloom/patchloom/commit/88d5fda3f22516915e66cf33f8a9d00335123ebd))
* **ci:** upload Sigstore attestation bundles to GitHub Releases ([#466](https://github.com/patchloom/patchloom/issues/466)) ([2496409](https://github.com/patchloom/patchloom/commit/2496409382227576dcb997ed0c5a0c995b571f4d))
* **ci:** use App token in update-branches to trigger CI on updated PRs ([#523](https://github.com/patchloom/patchloom/issues/523)) ([e51cdae](https://github.com/patchloom/patchloom/commit/e51cdae6ac200ac443ec1bc923b3c9c27c02a3e3))
* colored diff output, edge-case tests, and clearer error messages ([#468](https://github.com/patchloom/patchloom/issues/468)) ([2eb5e39](https://github.com/patchloom/patchloom/commit/2eb5e394c0124446f7ae796ac59de4872cdebfee))
* complete driver refactoring (2 missed call sites, restore Path imports) ([7f389f8](https://github.com/patchloom/patchloom/commit/7f389f876df79cadca03bec00ea86077fb2d7cca))
* correct pinned action SHAs in docs workflow ([#549](https://github.com/patchloom/patchloom/issues/549)) ([b1fabf6](https://github.com/patchloom/patchloom/commit/b1fabf6895ec73560d7d380c6bc6a5f82469741c))
* cross-platform backup paths for Windows drive letters ([334ecb4](https://github.com/patchloom/patchloom/commit/334ecb4a32db4e8bf21ced38c2e2f6acf6665062))
* **doc:** correct predicate syntax in doc select help text ([84471b9](https://github.com/patchloom/patchloom/commit/84471b96d5986a95a2d96d3f1c8532ce79d1b815))
* eliminate flaky validation tests caused by timestamp collision ([f112d97](https://github.com/patchloom/patchloom/commit/f112d97974e16797dec5cdc6ce7de608b61a807d))
* gate AstRename/AstReplace match arm behind cfg(feature = "ast") ([#680](https://github.com/patchloom/patchloom/issues/680)) ([ceea1f9](https://github.com/patchloom/patchloom/commit/ceea1f9e6669713d9697699d8dbd001296c242b9)), closes [#679](https://github.com/patchloom/patchloom/issues/679)
* guard ast cfg, execute_plan guard tests, tx smell clean (batch [#755](https://github.com/patchloom/patchloom/issues/755) follow-up) ([#763](https://github.com/patchloom/patchloom/issues/763)) ([d93e9d0](https://github.com/patchloom/patchloom/commit/d93e9d04f9fc625ddd08abf828b740d8adc16050))
* honor --format in tx + improve undo errors (MPI) ([#745](https://github.com/patchloom/patchloom/issues/745)) ([062a917](https://github.com/patchloom/patchloom/commit/062a91744792f4efb5ca81a0423065e865498304))
* ignore .lycheecache and add `make git-clean` (addresses tech-debt [#736](https://github.com/patchloom/patchloom/issues/736)) ([#741](https://github.com/patchloom/patchloom/issues/741)) ([b133614](https://github.com/patchloom/patchloom/commit/b1336145f52ace5d6cd222b70b25dad5185cf00e))
* improvement cycle (UTF-8 truncate, doc_set double-parse, docs freshness) ([#531](https://github.com/patchloom/patchloom/issues/531)) ([a8dffb9](https://github.com/patchloom/patchloom/commit/a8dffb9c8a5c1588dfa7b9a0f6d003772e41b6d4))
* improvement cycle 1 (create backup, tidy JSON, finalize ordering) ([#428](https://github.com/patchloom/patchloom/issues/428)) ([cabd164](https://github.com/patchloom/patchloom/commit/cabd164f38af7ab5f1ae5992edb0d98eca8cdc9e))
* improvement cycle 11 — config, schema, MCP tests, docs ([#568](https://github.com/patchloom/patchloom/issues/568)) ([ea4967b](https://github.com/patchloom/patchloom/commit/ea4967bc53f0d123fbdb6c9336a53f66638ab3be))
* improvement cycle 11b - docs, CI hardening ([#569](https://github.com/patchloom/patchloom/issues/569)) ([5041287](https://github.com/patchloom/patchloom/commit/5041287207d695f45e82200b063b39ae3e6f4159))
* improvement cycle 12 - Windows CI, fuzz CI matrix ([#572](https://github.com/patchloom/patchloom/issues/572)) ([c24792f](https://github.com/patchloom/patchloom/commit/c24792fe51a540c6afb2e8f66cf2f54648b561fe))
* improvement cycle 13 - tests, inline refactor, error context ([#575](https://github.com/patchloom/patchloom/issues/575)) ([6208177](https://github.com/patchloom/patchloom/commit/6208177ad64228b4278310f39a4f23ccab50068b))
* improvement cycle 14 - strengthen weak test assertions ([#577](https://github.com/patchloom/patchloom/issues/577)) ([2ba2396](https://github.com/patchloom/patchloom/commit/2ba2396ea310c7ccf78913dcfe1e82ca5610e311))
* improvement cycle 19 - PTY flush, MCP tests, doc updates ([#670](https://github.com/patchloom/patchloom/issues/670)) ([af289e9](https://github.com/patchloom/patchloom/commit/af289e9c76a9d119b1c12876d76400a99339d7b5))
* improvement cycle 2 (delete backup, tidy exit code tests) ([#429](https://github.com/patchloom/patchloom/issues/429)) ([50f58c5](https://github.com/patchloom/patchloom/commit/50f58c5fa9bbedf1059d1ee493267d15631c600a))
* improvement cycle 3 (backup consistency, test coverage) ([#430](https://github.com/patchloom/patchloom/issues/430)) ([8486726](https://github.com/patchloom/patchloom/commit/8486726b4e4a05a8f23f97250fce3794863f5552))
* improvement cycle 4 (MCP tests, doc dedup, error messages) ([#507](https://github.com/patchloom/patchloom/issues/507)) ([764c355](https://github.com/patchloom/patchloom/commit/764c3554c219ee7d5ca9c9098c73b0621ff90ad9))
* improvement cycle 5 (tx.rs refactoring, error path tests) ([#508](https://github.com/patchloom/patchloom/issues/508)) ([680b18b](https://github.com/patchloom/patchloom/commit/680b18bbd78f00a1eccaff7026b5292a178ebea9))
* improvement cycle 6 (doc_query validation, troubleshooting docs) ([#520](https://github.com/patchloom/patchloom/issues/520)) ([93d3fdf](https://github.com/patchloom/patchloom/commit/93d3fdf77957d0fa14dc9f358c39a402a2f0af6c))
* install lychee from GitHub releases on ubuntu-latest ([6c2d3bc](https://github.com/patchloom/patchloom/commit/6c2d3bc3361d2c44f3a923af4c417d6bd62f8a50))
* isolate Trivy from runner's broken Docker credential helper ([14734d2](https://github.com/patchloom/patchloom/commit/14734d26a81b5f1329f7fa5321d497d5ca7effc1))
* make release host job idempotent for release-please ([#511](https://github.com/patchloom/patchloom/issues/511)) ([2b6ae3b](https://github.com/patchloom/patchloom/commit/2b6ae3b2507282d2257906bc5c35a542ceb2e4dc))
* make unit tests portable in Docker and pseudo-TTY environments ([#579](https://github.com/patchloom/patchloom/issues/579)) ([591b4d8](https://github.com/patchloom/patchloom/commit/591b4d83db426ff7cea6c69926698e5bd3182d15))
* make update-readme portable across BSD and GNU sed ([3d60525](https://github.com/patchloom/patchloom/commit/3d60525ae2542aa8a276a84c598976842586e460)), closes [#360](https://github.com/patchloom/patchloom/issues/360)
* MCP validation parity, config boundary, empty file, cross-file md_move ([#712](https://github.com/patchloom/patchloom/issues/712)) ([9c37f01](https://github.com/patchloom/patchloom/commit/9c37f0180f578ab2ef809c3fa556446269ca82bf))
* **mcp:** remove batch/transaction tools for zero-failure agent benchmarks ([#481](https://github.com/patchloom/patchloom/issues/481)) ([1ea3849](https://github.com/patchloom/patchloom/commit/1ea3849df96b0f11a110e3b17c8abfd66fcfaeba))
* md move-section same-file path detection and cross-file --check mode ([#556](https://github.com/patchloom/patchloom/issues/556)) ([da76cc5](https://github.com/patchloom/patchloom/commit/da76cc5cb0ce1ecfee8027ba7b7d1c3d6a577bdf))
* md silent default mode, search empty-pattern guard, strengthen assertions ([#542](https://github.com/patchloom/patchloom/issues/542)) ([45d3239](https://github.com/patchloom/patchloom/commit/45d323976bdc19e4bb9d37f23ba60566f0dc43a9))
* md/doc --check produce stdout output and doc --json errors use structured JSON ([#546](https://github.com/patchloom/patchloom/issues/546)) ([819fb7c](https://github.com/patchloom/patchloom/commit/819fb7c1a2190e74445672a1dbb3c77f09496e9a)), closes [#544](https://github.com/patchloom/patchloom/issues/544) [#545](https://github.com/patchloom/patchloom/issues/545)
* move VS Code badge to distribution row to prevent 4-row wrap ([#643](https://github.com/patchloom/patchloom/issues/643)) ([3461e2d](https://github.com/patchloom/patchloom/commit/3461e2d67e0c4d3ab65a98c12fdac2c7a2156a56))
* parse release-please pr output as JSON ([#515](https://github.com/patchloom/patchloom/issues/515)) ([3215fcd](https://github.com/patchloom/patchloom/commit/3215fcdf2137ccf6a2243b7a8373d58a0f0ad94b))
* preserve single-file text format in tx search, add path-prefix assertions ([54f420f](https://github.com/patchloom/patchloom/commit/54f420f5d30e259027b88590a08eecba74ea85b1))
* prevent data corruption when backing up files outside project root ([be4bf78](https://github.com/patchloom/patchloom/commit/be4bf78606ab1281d427c7f1b411f46c7dda636e)), closes [#373](https://github.com/patchloom/patchloom/issues/373)
* propagate backup finalize errors instead of discarding them ([35d65f8](https://github.com/patchloom/patchloom/commit/35d65f8542f2e54c786ef22c0e20fe965d01b4e8))
* propagate read errors in file_create and extract inline conditional ([#533](https://github.com/patchloom/patchloom/issues/533)) ([26ab09c](https://github.com/patchloom/patchloom/commit/26ab09cca8c5a3229a4de6350137aded69e4ec1a))
* propagate YAML serialization error and remove unnecessary borrows in ops.rs ([#537](https://github.com/patchloom/patchloom/issues/537)) ([24e67f4](https://github.com/patchloom/patchloom/commit/24e67f40755606863add7d83468a28583a42f7d5))
* re-expose fallback and AST internals as public API for library consumers ([#677](https://github.com/patchloom/patchloom/issues/677)) ([c25ce76](https://github.com/patchloom/patchloom/commit/c25ce766b4d5605d2284136a964be27bb07cbc08))
* rebalance badge rows to 5-5-3 to prevent row 2 wrap ([#644](https://github.com/patchloom/patchloom/issues/644)) ([bdc5da0](https://github.com/patchloom/patchloom/commit/bdc5da03aaedc429dd7a257a210c68f16b0cabb4))
* remove bump-patch-for-minor-pre-major to align with semver-checks ([#672](https://github.com/patchloom/patchloom/issues/672)) ([f930c9b](https://github.com/patchloom/patchloom/commit/f930c9bc90d53a9ba466ebedf81bd9546c190305))
* remove dead test code in containment path guard ([#628](https://github.com/patchloom/patchloom/issues/628)) ([5e5b9e5](https://github.com/patchloom/patchloom/commit/5e5b9e5fa459dc60bcc124565471dd9debd4afb2))
* remove documentation field so crates.io auto-links to docs.rs ([#547](https://github.com/patchloom/patchloom/issues/547)) ([f6bbd10](https://github.com/patchloom/patchloom/commit/f6bbd10d30d60c6964d68a8d45d2c72ed14aaa1a))
* remove unused import and write bench summary to step summary ([bed246c](https://github.com/patchloom/patchloom/commit/bed246cea8c4a289956810fef3a77f54a9090e54))
* rename misleading concurrent MCP test and clean cosmetic GlobalFlags shadows ([#721](https://github.com/patchloom/patchloom/issues/721)) ([3a6918e](https://github.com/patchloom/patchloom/commit/3a6918e7c8e6fc25917503d9838a0d2d736b6cce))
* rename same-file detection via path canonicalization ([#557](https://github.com/patchloom/patchloom/issues/557)) ([a1b5573](https://github.com/patchloom/patchloom/commit/a1b5573a573744ebcd5806beae187e8e232ec5aa))
* replace broken shields.io badges with gist endpoints ([#578](https://github.com/patchloom/patchloom/issues/578)) ([23b14f3](https://github.com/patchloom/patchloom/commit/23b14f389a12c8d044cc79cb29ff6eb1b751f3de))
* replace retired VS Code Marketplace badge with gist endpoint ([#641](https://github.com/patchloom/patchloom/issues/641)) ([2b30ab6](https://github.com/patchloom/patchloom/commit/2b30ab6195377510f59c1bc6f4676218d27606b9))
* replace stale 'key' with 'selector' in all descriptions and docs ([ede2e1d](https://github.com/patchloom/patchloom/commit/ede2e1d903a8d463ce225f06194dd8c1134955b9))
* **replace:** include search path in no-match stderr message ([37f919a](https://github.com/patchloom/patchloom/commit/37f919a8b2254193b1d5872f9cfaf4514a5f4ec7))
* resolve 26 CodeQL Python quality findings ([#439](https://github.com/patchloom/patchloom/issues/439)) ([31a9d5d](https://github.com/patchloom/patchloom/commit/31a9d5d9745bae7b8027449ddc9e7951ff31ef89))
* resolve 5 open issues ([#409](https://github.com/patchloom/patchloom/issues/409)-[#413](https://github.com/patchloom/patchloom/issues/413)) ([75c8e82](https://github.com/patchloom/patchloom/commit/75c8e82afa9df5c38062571507cb1ee0ebdbdcb5)), closes [#410](https://github.com/patchloom/patchloom/issues/410) [#411](https://github.com/patchloom/patchloom/issues/411) [#412](https://github.com/patchloom/patchloom/issues/412)
* resolve cyclic imports and restore dynamic coverage badge ([#442](https://github.com/patchloom/patchloom/issues/442)) ([251287c](https://github.com/patchloom/patchloom/commit/251287c277f071af0eb8303de78901c511dc7cb2))
* resolve GitHub AI code quality findings ([#469](https://github.com/patchloom/patchloom/issues/469)) ([abfdbdb](https://github.com/patchloom/patchloom/commit/abfdbdbeec59cbb36ec8f5af5edb5497321a488d))
* resolve issues [#364](https://github.com/patchloom/patchloom/issues/364)-367 from Cycle 3 ([31e546b](https://github.com/patchloom/patchloom/commit/31e546b99cdb0ada383fbe976eea3afa6c067a87)), closes [#365](https://github.com/patchloom/patchloom/issues/365) [#366](https://github.com/patchloom/patchloom/issues/366) [#367](https://github.com/patchloom/patchloom/issues/367)
* resolve tech-debt [#691](https://github.com/patchloom/patchloom/issues/691), [#694](https://github.com/patchloom/patchloom/issues/694), [#708](https://github.com/patchloom/patchloom/issues/708) (clap optional, AST tests, fuzz) ([#714](https://github.com/patchloom/patchloom/issues/714)) ([e116430](https://github.com/patchloom/patchloom/commit/e1164304334e50e0971bce8a085229559d04ebbc))
* resolve tech-debt issues [#620](https://github.com/patchloom/patchloom/issues/620)-[#623](https://github.com/patchloom/patchloom/issues/623) ([#625](https://github.com/patchloom/patchloom/issues/625)) ([b031686](https://github.com/patchloom/patchloom/commit/b03168619872d77cf5963892aac728c725b93768)), closes [#621](https://github.com/patchloom/patchloom/issues/621) [#622](https://github.com/patchloom/patchloom/issues/622)
* reviewer follow-ups for 755-759 batch (ast cfg guards, md_move Apply-mode checks) ([#761](https://github.com/patchloom/patchloom/issues/761)) ([c7f56bf](https://github.com/patchloom/patchloom/commit/c7f56bf31a4b9848a5ce3b8b5e30bd769ab91d0b))
* run --format command in --confirm paths for all write commands ([#668](https://github.com/patchloom/patchloom/issues/668)) ([c884f75](https://github.com/patchloom/patchloom/commit/c884f753e29dfd3686ab4c394869ecdfab1bc4ee))
* run plan format/validate lifecycle on tx --confirm + batch cleanup (fixes [#744](https://github.com/patchloom/patchloom/issues/744)) ([#747](https://github.com/patchloom/patchloom/issues/747)) ([3a9a26c](https://github.com/patchloom/patchloom/commit/3a9a26c06f2c3d1ff049983b16feaa3015c97382))
* **schema:** add op field to md.move_section examples ([#600](https://github.com/patchloom/patchloom/issues/600)) ([9a816fe](https://github.com/patchloom/patchloom/commit/9a816fe955207ecfe322b9eb96232f474dee8d35))
* **selector:** reject ? prefix in predicate keys with helpful message ([723cae5](https://github.com/patchloom/patchloom/commit/723cae51a7c2fcdfc3e82435586476240fe9bf04)), closes [#403](https://github.com/patchloom/patchloom/issues/403)
* text and path matching bugs ([#685](https://github.com/patchloom/patchloom/issues/685), [#700](https://github.com/patchloom/patchloom/issues/700), [#701](https://github.com/patchloom/patchloom/issues/701)) ([#710](https://github.com/patchloom/patchloom/issues/710)) ([2f587d0](https://github.com/patchloom/patchloom/commit/2f587d0dbf94f184469675e98d134161cc6aeea9))
* transaction engine atomicity and rename-after-delete ([#696](https://github.com/patchloom/patchloom/issues/696), [#697](https://github.com/patchloom/patchloom/issues/697), [#698](https://github.com/patchloom/patchloom/issues/698)) ([#711](https://github.com/patchloom/patchloom/issues/711)) ([fe51c17](https://github.com/patchloom/patchloom/commit/fe51c17dd2200b9a245e97d71e5027b6ccb1bfe8))
* undo correctly restores files that were outside the project root ([5d4b397](https://github.com/patchloom/patchloom/commit/5d4b397ae61b15008a38dbfa1a1d50892282e88c))
* unique OPERATION_FAILED exit code, timestamp-based pruning, AST improvements ([#713](https://github.com/patchloom/patchloom/issues/713)) ([572fb08](https://github.com/patchloom/patchloom/commit/572fb0886465d289bca6987f1fb3fdd4cce83d80))
* update bench.yml upload-artifact to v7, add concurrency group ([4cbb7e9](https://github.com/patchloom/patchloom/commit/4cbb7e9cc62682af3fdb7dd88419013ef8dde52e))
* update MCP bench to use individual tool calls ([#570](https://github.com/patchloom/patchloom/issues/570)) ([655a1d2](https://github.com/patchloom/patchloom/commit/655a1d24b7d9e89c73d9f91a852957a2a8327681))
* update stale test counts in README and agent test docs ([d57bc18](https://github.com/patchloom/patchloom/commit/d57bc182d70f7802442ea87886c93bbc22269afd))
* use .intoto.jsonl extension for attestation bundles ([#467](https://github.com/patchloom/patchloom/issues/467)) ([7c88e80](https://github.com/patchloom/patchloom/commit/7c88e80e3a8a452421a99e883e95f358af17f504))
* use correct lychee release tag and asset name ([05a6d72](https://github.com/patchloom/patchloom/commit/05a6d722626336d93ac65e50d540b538c942eddd))
* use ghcr.io for Trivy DB to avoid GCR credential errors ([78d7b74](https://github.com/patchloom/patchloom/commit/78d7b74ffae79900cb78698f327dd743a9b77bf6))
* use nanosecond timestamps in backup sessions ([5e1962a](https://github.com/patchloom/patchloom/commit/5e1962a5de0b814a73740039a4962689d91527e7)), closes [#363](https://github.com/patchloom/patchloom/issues/363)
* use platform-appropriate absolute paths in containment tests ([#616](https://github.com/patchloom/patchloom/issues/616)) ([6a0bb6a](https://github.com/patchloom/patchloom/commit/6a0bb6ae7904de09ac2a3e64042c5c6495c20270))
* use streaming binary probe in tx dir search to avoid large allocations ([a05b639](https://github.com/patchloom/patchloom/commit/a05b639dd87ecbefd09efc22cebc79be3fcb14e0))
* use thread-local FORCE_RESTORE_FAIL for parallel tests ([#594](https://github.com/patchloom/patchloom/issues/594)) ([57b6f9b](https://github.com/patchloom/patchloom/commit/57b6f9bd00f123b8308ba2f81c90ee2a8ec33930))
* warn on invalid config values and clarify batch quoting ([#585](https://github.com/patchloom/patchloom/issues/585)) ([7803291](https://github.com/patchloom/patchloom/commit/7803291f7523c2d1dc684b73cd4148a3d6c74286))
* warn on malformed .patchloom.toml, add backup pruning tests and troubleshooting docs ([2e300d5](https://github.com/patchloom/patchloom/commit/2e300d523c315a7b388d3828e16736b81defbfed)), closes [#369](https://github.com/patchloom/patchloom/issues/369) [#371](https://github.com/patchloom/patchloom/issues/371) [#372](https://github.com/patchloom/patchloom/issues/372)
* Windows backup test failures (directory open + external path prefix) ([be64e12](https://github.com/patchloom/patchloom/commit/be64e1288de5f10335568623add9fc1bb1fc441b))
* wire prune_old_backups into backup session creation ([2709ce7](https://github.com/patchloom/patchloom/commit/2709ce7861c072850db1f08f759251f2853cad71))


### Performance Improvements

* cache canonicalized cwd in MCP server ([c0ef850](https://github.com/patchloom/patchloom/commit/c0ef850c9acf50fbea233c8d0b89218680918674))
* four targeted optimizations across hot paths ([3d0ab90](https://github.com/patchloom/patchloom/commit/3d0ab901b70d3704036c79cc551d6dcbfe100bf8))

## [0.4.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.3.0...patchloom-v0.4.0) (2026-06-21)

### Changed

* Restructured Cargo features for greater modularity and library reusability (primarily [#714](https://github.com/patchloom/patchloom/issues/714)):
  - Introduced the `cli` feature, making clap and related CLI machinery fully optional. This allows using patchloom as a pure library without pulling in CLI dependencies.
  - The previous coarse `core` feature has been replaced by a more granular model. Default is now `["cli", "mcp", "ast"]`. Users can select exactly the capabilities they need (or use `default-features = false` for a minimal footprint).
  - `full` now aliases to `["cli", "mcp", "ast"]`.
* Moved some types (e.g. `LintIssue`) out of `cmd::md` into `ops::md` (and re-exported via the public API) to make core editing functionality more directly reusable outside of command implementations.

### ⚠ BREAKING CHANGES

* The `core` feature no longer exists. Use explicit feature selection (`cli`, `mcp`, `ast`, `full`, or combinations) instead.
* The path `patchloom::cmd::md::LintIssue` has changed; it is now at `patchloom::ops::md::LintIssue` (re-exported publicly).

The goal of this work was to make more of the crate independently usable and reduce unnecessary dependencies for library consumers, even though the exact feature names and some public paths changed.

## [0.3.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.2.0...patchloom-v0.3.0) (2026-06-21)


### Features

* add --word-boundary flag to prevent partial-word matches ([#657](https://github.com/patchloom/patchloom/issues/657)) ([91893e2](https://github.com/patchloom/patchloom/commit/91893e2cafc1f615904665e83cf7dabd12d6e8ba))
* add AST-aware operations using tree-sitter (20 languages) ([#658](https://github.com/patchloom/patchloom/issues/658)) ([17223e5](https://github.com/patchloom/patchloom/commit/17223e515a9e625999bc5dbff454b2ea2ec2b0df)), closes [#647](https://github.com/patchloom/patchloom/issues/647)
* add file.append operation and --format flag on write commands ([#667](https://github.com/patchloom/patchloom/issues/667)) ([9a57f06](https://github.com/patchloom/patchloom/commit/9a57f06d58ac979701a3bfda9b3a8c7c514cf814)), closes [#661](https://github.com/patchloom/patchloom/issues/661) [#662](https://github.com/patchloom/patchloom/issues/662)
* **ast:** add map, replace, impact, and diff subcommands ([#650](https://github.com/patchloom/patchloom/issues/650), [#653](https://github.com/patchloom/patchloom/issues/653), [#654](https://github.com/patchloom/patchloom/issues/654), [#655](https://github.com/patchloom/patchloom/issues/655)) ([#660](https://github.com/patchloom/patchloom/issues/660)) ([d4eef5a](https://github.com/patchloom/patchloom/commit/d4eef5a17db9891cabdfbd265cbabdfa2534c588))
* **ast:** add search, refs, and deps subcommands ([#649](https://github.com/patchloom/patchloom/issues/649), [#651](https://github.com/patchloom/patchloom/issues/651), [#652](https://github.com/patchloom/patchloom/issues/652)) ([#659](https://github.com/patchloom/patchloom/issues/659)) ([72eccb8](https://github.com/patchloom/patchloom/commit/72eccb83405e54ce92676c4be485ad1e0779c8b6))


### Bug Fixes

* add AST and word_boundary to all operation surfaces ([#666](https://github.com/patchloom/patchloom/issues/666)) ([ff85bb7](https://github.com/patchloom/patchloom/commit/ff85bb756654dd2504b78242685c8b9b7658905f)), closes [#663](https://github.com/patchloom/patchloom/issues/663) [#664](https://github.com/patchloom/patchloom/issues/664) [#665](https://github.com/patchloom/patchloom/issues/665)
* **ci:** remove dependabot[bot] from auto-approve actor list ([#648](https://github.com/patchloom/patchloom/issues/648)) ([677982d](https://github.com/patchloom/patchloom/commit/677982dae876dd9c3f1b2682af38d1657027381a))
* improvement cycle 19 - PTY flush, MCP tests, doc updates ([#670](https://github.com/patchloom/patchloom/issues/670)) ([af289e9](https://github.com/patchloom/patchloom/commit/af289e9c76a9d119b1c12876d76400a99339d7b5))
* move VS Code badge to distribution row to prevent 4-row wrap ([#643](https://github.com/patchloom/patchloom/issues/643)) ([3461e2d](https://github.com/patchloom/patchloom/commit/3461e2d67e0c4d3ab65a98c12fdac2c7a2156a56))
* re-expose fallback and AST internals as public API for library consumers ([#677](https://github.com/patchloom/patchloom/issues/677)) ([c25ce76](https://github.com/patchloom/patchloom/commit/c25ce766b4d5605d2284136a964be27bb07cbc08))
* rebalance badge rows to 5-5-3 to prevent row 2 wrap ([#644](https://github.com/patchloom/patchloom/issues/644)) ([bdc5da0](https://github.com/patchloom/patchloom/commit/bdc5da03aaedc429dd7a257a210c68f16b0cabb4))
* remove bump-patch-for-minor-pre-major to align with semver-checks ([#672](https://github.com/patchloom/patchloom/issues/672)) ([f930c9b](https://github.com/patchloom/patchloom/commit/f930c9bc90d53a9ba466ebedf81bd9546c190305))
* replace retired VS Code Marketplace badge with gist endpoint ([#641](https://github.com/patchloom/patchloom/issues/641)) ([2b30ab6](https://github.com/patchloom/patchloom/commit/2b30ab6195377510f59c1bc6f4676218d27606b9))
* run --format command in --confirm paths for all write commands ([#668](https://github.com/patchloom/patchloom/issues/668)) ([c884f75](https://github.com/patchloom/patchloom/commit/c884f753e29dfd3686ab4c394869ecdfab1bc4ee))

## [0.2.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.7...patchloom-v0.2.0) (2026-06-18)


### ⚠ BREAKING CHANGES

* All public structs and enums in the API surface are now marked #[non_exhaustive]. External code that constructs these types via struct literals must use ..Default::default() or equivalent patterns. Serde deserialization (the primary construction path) is unaffected.

### Features

* add cargo-semver-checks CI and fix all rustdoc warnings ([#615](https://github.com/patchloom/patchloom/issues/615)) ([250dcb5](https://github.com/patchloom/patchloom/commit/250dcb52191b22ca3beda879715ac7000dd306a7)), closes [#612](https://github.com/patchloom/patchloom/issues/612) [#613](https://github.com/patchloom/patchloom/issues/613)
* extract path containment into public module ([#609](https://github.com/patchloom/patchloom/issues/609)) ([d3d9ae2](https://github.com/patchloom/patchloom/commit/d3d9ae26e2b07c9867a3ac38461c637d84e6bd44))
* harden tx rollback and add three-way patch merge ([#587](https://github.com/patchloom/patchloom/issues/587)) ([db21982](https://github.com/patchloom/patchloom/commit/db2198222aa159d4b8b4874e14c4dbd399569909))
* make files module public and extract exec module ([#610](https://github.com/patchloom/patchloom/issues/610)) ([746701a](https://github.com/patchloom/patchloom/commit/746701a9030ac6dc81ce78ef1594e33bcfb8fe6f))
* mark all public types as #[non_exhaustive] for semver safety ([#624](https://github.com/patchloom/patchloom/issues/624)) ([b3592e2](https://github.com/patchloom/patchloom/commit/b3592e20ead71c62f6bc302bee432182447f0fed))
* re-export EolMode from write module ([#611](https://github.com/patchloom/patchloom/issues/611)) ([fc604d9](https://github.com/patchloom/patchloom/commit/fc604d9b60963e73d1cef461c1cb1899648b0564))
* support RELEASE_NOTES.md override for curated release descriptions ([#627](https://github.com/patchloom/patchloom/issues/627)) ([f0f92be](https://github.com/patchloom/patchloom/commit/f0f92be348c2371ad625770cd260092a077c12b8))


### Bug Fixes

* remove dead test code in containment path guard ([#628](https://github.com/patchloom/patchloom/issues/628)) ([5e5b9e5](https://github.com/patchloom/patchloom/commit/5e5b9e5fa459dc60bcc124565471dd9debd4afb2))
* resolve tech-debt issues [#620](https://github.com/patchloom/patchloom/issues/620)-[#623](https://github.com/patchloom/patchloom/issues/623) ([#625](https://github.com/patchloom/patchloom/issues/625)) ([b031686](https://github.com/patchloom/patchloom/commit/b03168619872d77cf5963892aac728c725b93768)), closes [#621](https://github.com/patchloom/patchloom/issues/621) [#622](https://github.com/patchloom/patchloom/issues/622)
* **schema:** add op field to md.move_section examples ([#600](https://github.com/patchloom/patchloom/issues/600)) ([9a816fe](https://github.com/patchloom/patchloom/commit/9a816fe955207ecfe322b9eb96232f474dee8d35))
* use platform-appropriate absolute paths in containment tests ([#616](https://github.com/patchloom/patchloom/issues/616)) ([6a0bb6a](https://github.com/patchloom/patchloom/commit/6a0bb6ae7904de09ac2a3e64042c5c6495c20270))
* use thread-local FORCE_RESTORE_FAIL for parallel tests ([#594](https://github.com/patchloom/patchloom/issues/594)) ([57b6f9b](https://github.com/patchloom/patchloom/commit/57b6f9bd00f123b8308ba2f81c90ee2a8ec33930))
* warn on invalid config values and clarify batch quoting ([#585](https://github.com/patchloom/patchloom/issues/585)) ([7803291](https://github.com/patchloom/patchloom/commit/7803291f7523c2d1dc684b73cd4148a3d6c74286))

## [0.1.7](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.6...patchloom-v0.1.7) (2026-06-16)


### Features

* add --whole-line, --range, and --collapse-blanks to replace ([#564](https://github.com/patchloom/patchloom/issues/564)) ([5651320](https://github.com/patchloom/patchloom/commit/56513207a0c7f7c18b3745825fc369eb04cc1271)), closes [#563](https://github.com/patchloom/patchloom/issues/563)
* close [#573](https://github.com/patchloom/patchloom/issues/573) and [#574](https://github.com/patchloom/patchloom/issues/574) - complete API parity and edge case tests ([#576](https://github.com/patchloom/patchloom/issues/576)) ([d6fc1a9](https://github.com/patchloom/patchloom/commit/d6fc1a99fb1f36045bd309a4707d8f4a84919bb5))
* md.move-section -- move a heading section between files ([#554](https://github.com/patchloom/patchloom/issues/554)) ([d6f42e7](https://github.com/patchloom/patchloom/commit/d6f42e7e97db115d3506ab8295c4e261aee2f67e)), closes [#553](https://github.com/patchloom/patchloom/issues/553)


### Bug Fixes

* improvement cycle 11 — config, schema, MCP tests, docs ([#568](https://github.com/patchloom/patchloom/issues/568)) ([ea4967b](https://github.com/patchloom/patchloom/commit/ea4967bc53f0d123fbdb6c9336a53f66638ab3be))
* improvement cycle 11b - docs, CI hardening ([#569](https://github.com/patchloom/patchloom/issues/569)) ([5041287](https://github.com/patchloom/patchloom/commit/5041287207d695f45e82200b063b39ae3e6f4159))
* improvement cycle 12 - Windows CI, fuzz CI matrix ([#572](https://github.com/patchloom/patchloom/issues/572)) ([c24792f](https://github.com/patchloom/patchloom/commit/c24792fe51a540c6afb2e8f66cf2f54648b561fe))
* improvement cycle 13 - tests, inline refactor, error context ([#575](https://github.com/patchloom/patchloom/issues/575)) ([6208177](https://github.com/patchloom/patchloom/commit/6208177ad64228b4278310f39a4f23ccab50068b))
* improvement cycle 14 - strengthen weak test assertions ([#577](https://github.com/patchloom/patchloom/issues/577)) ([2ba2396](https://github.com/patchloom/patchloom/commit/2ba2396ea310c7ccf78913dcfe1e82ca5610e311))
* make unit tests portable in Docker and pseudo-TTY environments ([#579](https://github.com/patchloom/patchloom/issues/579)) ([591b4d8](https://github.com/patchloom/patchloom/commit/591b4d83db426ff7cea6c69926698e5bd3182d15))
* md move-section same-file path detection and cross-file --check mode ([#556](https://github.com/patchloom/patchloom/issues/556)) ([da76cc5](https://github.com/patchloom/patchloom/commit/da76cc5cb0ce1ecfee8027ba7b7d1c3d6a577bdf))
* rename same-file detection via path canonicalization ([#557](https://github.com/patchloom/patchloom/issues/557)) ([a1b5573](https://github.com/patchloom/patchloom/commit/a1b5573a573744ebcd5806beae187e8e232ec5aa))
* replace broken shields.io badges with gist endpoints ([#578](https://github.com/patchloom/patchloom/issues/578)) ([23b14f3](https://github.com/patchloom/patchloom/commit/23b14f389a12c8d044cc79cb29ff6eb1b751f3de))
* update MCP bench to use individual tool calls ([#570](https://github.com/patchloom/patchloom/issues/570)) ([655a1d2](https://github.com/patchloom/patchloom/commit/655a1d24b7d9e89c73d9f91a852957a2a8327681))

## [0.1.6](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.5...patchloom-v0.1.6) (2026-06-08)


### Features

* public Rust library API with thread safety, intent format, and fallback chain ([#530](https://github.com/patchloom/patchloom/issues/530)) ([093eb8b](https://github.com/patchloom/patchloom/commit/093eb8bc0abf4d567027fd9a726934943823e1e2))


### Bug Fixes

* add error context to backup restore and rename cross-device paths ([#543](https://github.com/patchloom/patchloom/issues/543)) ([69018e7](https://github.com/patchloom/patchloom/commit/69018e784e9a5594b70000275167d15d67a1a0a0))
* **ci:** use App token in update-branches to trigger CI on updated PRs ([#523](https://github.com/patchloom/patchloom/issues/523)) ([e51cdae](https://github.com/patchloom/patchloom/commit/e51cdae6ac200ac443ec1bc923b3c9c27c02a3e3))
* correct pinned action SHAs in docs workflow ([#549](https://github.com/patchloom/patchloom/issues/549)) ([b1fabf6](https://github.com/patchloom/patchloom/commit/b1fabf6895ec73560d7d380c6bc6a5f82469741c))
* improvement cycle (UTF-8 truncate, doc_set double-parse, docs freshness) ([#531](https://github.com/patchloom/patchloom/issues/531)) ([a8dffb9](https://github.com/patchloom/patchloom/commit/a8dffb9c8a5c1588dfa7b9a0f6d003772e41b6d4))
* md silent default mode, search empty-pattern guard, strengthen assertions ([#542](https://github.com/patchloom/patchloom/issues/542)) ([45d3239](https://github.com/patchloom/patchloom/commit/45d323976bdc19e4bb9d37f23ba60566f0dc43a9))
* md/doc --check produce stdout output and doc --json errors use structured JSON ([#546](https://github.com/patchloom/patchloom/issues/546)) ([819fb7c](https://github.com/patchloom/patchloom/commit/819fb7c1a2190e74445672a1dbb3c77f09496e9a)), closes [#544](https://github.com/patchloom/patchloom/issues/544) [#545](https://github.com/patchloom/patchloom/issues/545)
* propagate read errors in file_create and extract inline conditional ([#533](https://github.com/patchloom/patchloom/issues/533)) ([26ab09c](https://github.com/patchloom/patchloom/commit/26ab09cca8c5a3229a4de6350137aded69e4ec1a))
* propagate YAML serialization error and remove unnecessary borrows in ops.rs ([#537](https://github.com/patchloom/patchloom/issues/537)) ([24e67f4](https://github.com/patchloom/patchloom/commit/24e67f40755606863add7d83468a28583a42f7d5))
* remove documentation field so crates.io auto-links to docs.rs ([#547](https://github.com/patchloom/patchloom/issues/547)) ([f6bbd10](https://github.com/patchloom/patchloom/commit/f6bbd10d30d60c6964d68a8d45d2c72ed14aaa1a))

## [0.1.5](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.4...patchloom-v0.1.5) (2026-06-07)


### Bug Fixes

* improvement cycle 6 (doc_query validation, troubleshooting docs) ([#520](https://github.com/patchloom/patchloom/issues/520)) ([93d3fdf](https://github.com/patchloom/patchloom/commit/93d3fdf77957d0fa14dc9f358c39a402a2f0af6c))

## [0.1.4](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.3...patchloom-v0.1.4) (2026-06-07)


### Bug Fixes

* auto-sync PATCHLOOM.md on release-please version bumps ([#513](https://github.com/patchloom/patchloom/issues/513)) ([cb6cb1c](https://github.com/patchloom/patchloom/commit/cb6cb1c3dca42974c2230485d3a712dd3ac05b75)), closes [#512](https://github.com/patchloom/patchloom/issues/512)
* parse release-please pr output as JSON ([#515](https://github.com/patchloom/patchloom/issues/515)) ([3215fcd](https://github.com/patchloom/patchloom/commit/3215fcdf2137ccf6a2243b7a8373d58a0f0ad94b))

## [0.1.3](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.2...patchloom-v0.1.3) (2026-06-07)


### Bug Fixes

* add wasi crate to FOSSA false positive filter ([#510](https://github.com/patchloom/patchloom/issues/510)) ([1882060](https://github.com/patchloom/patchloom/commit/18820609dcd0fea3062e70a7e173f10836682464))
* improvement cycle 5 (tx.rs refactoring, error path tests) ([#508](https://github.com/patchloom/patchloom/issues/508)) ([680b18b](https://github.com/patchloom/patchloom/commit/680b18bbd78f00a1eccaff7026b5292a178ebea9))
* make release host job idempotent for release-please ([#511](https://github.com/patchloom/patchloom/issues/511)) ([2b6ae3b](https://github.com/patchloom/patchloom/commit/2b6ae3b2507282d2257906bc5c35a542ceb2e4dc))

## [0.1.2](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.1...patchloom-v0.1.2) (2026-06-07)


### Features

* enable MCP feature by default ([#502](https://github.com/patchloom/patchloom/issues/502)) ([7eb8750](https://github.com/patchloom/patchloom/commit/7eb87507e5b686a1d80391e50a97ce50abdd51a0))


### Bug Fixes

* add benchmark result directories to .gitignore ([#500](https://github.com/patchloom/patchloom/issues/500)) ([9bdd383](https://github.com/patchloom/patchloom/commit/9bdd38319f7fe2713e4b22eea24fa99244a9d392))
* improvement cycle 4 (MCP tests, doc dedup, error messages) ([#507](https://github.com/patchloom/patchloom/issues/507)) ([764c355](https://github.com/patchloom/patchloom/commit/764c3554c219ee7d5ca9c9098c73b0621ff90ad9))

## [0.1.1](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.0...patchloom-v0.1.1) (2026-06-06)


### Features

* add --confirm flag for interactive preview-then-apply workflow ([c7c0796](https://github.com/patchloom/patchloom/commit/c7c07966fb2331e1ec1bffcf5469341af4fde040)), closes [#354](https://github.com/patchloom/patchloom/issues/354)
* add 7 missing batch operations and 2 MCP tools ([20c6ce5](https://github.com/patchloom/patchloom/commit/20c6ce542753a00f468572fc05fc4030dcec39a7)), closes [#219](https://github.com/patchloom/patchloom/issues/219) [#223](https://github.com/patchloom/patchloom/issues/223)
* add batch command for line-oriented multi-file edits ([0c446b9](https://github.com/patchloom/patchloom/commit/0c446b94b15a3222620dbfa508dcb60d491ba4df))
* add Claude Code and Aider agent drivers ([75a33c3](https://github.com/patchloom/patchloom/commit/75a33c3f0c6c990ac58d6c1e0111fba001ce2424))
* add Codex CLI and Cline agent drivers ([97281d7](https://github.com/patchloom/patchloom/commit/97281d72100a69c84ff44ce3b591633434a68683))
* add inline examples to MCP tool descriptions and simplify bench prompts ([#479](https://github.com/patchloom/patchloom/issues/479)) ([fbd8336](https://github.com/patchloom/patchloom/commit/fbd83363b6a990b13034fa0929a545470693ae75))
* add MCP benchmark suite (make bench-mcp) ([#470](https://github.com/patchloom/patchloom/issues/470)) ([dc81fdd](https://github.com/patchloom/patchloom/commit/dc81fdd2d32f91e93abfa602d3a9d7bcf7206c0a))
* add MCP server for structured tool calls ([d039043](https://github.com/patchloom/patchloom/commit/d0390431dfc8e0debf5a99c2347ee6b88244aa6a))
* add patchloom explain command for human-readable plan descriptions ([f1a0056](https://github.com/patchloom/patchloom/commit/f1a00568cd19c90f7c800dbf627799a865d59b6d)), closes [#356](https://github.com/patchloom/patchloom/issues/356)
* add usage examples to --help output for all commands ([85bc1a6](https://github.com/patchloom/patchloom/commit/85bc1a6b655d76061c1b7e5750476a5c47a6ab33)), closes [#352](https://github.com/patchloom/patchloom/issues/352)
* add usage examples to MCP agent-rules output ([#471](https://github.com/patchloom/patchloom/issues/471)) ([e6eb618](https://github.com/patchloom/patchloom/commit/e6eb618a3b2b27837da3abe481344dec2a30d706))
* **agent-rules:** add --mode and --platform flags, fix Windows quoting ([4743892](https://github.com/patchloom/patchloom/commit/47438921dc936b9a6d0d310226ff439357f79828)), closes [#256](https://github.com/patchloom/patchloom/issues/256) [#257](https://github.com/patchloom/patchloom/issues/257) [#258](https://github.com/patchloom/patchloom/issues/258)
* auto-install shell completions in patchloom init ([019e793](https://github.com/patchloom/patchloom/commit/019e7938ee64ac47c4fc31464cc95e69dd592bc6)), closes [#353](https://github.com/patchloom/patchloom/issues/353)
* benchmark reproducibility (README, dry-run, report, CI) ([fa96a98](https://github.com/patchloom/patchloom/commit/fa96a982973d1941b29a023325020077656a7f88)), closes [#346](https://github.com/patchloom/patchloom/issues/346)
* comment preservation for sequence-rooted YAML files ([b5ca605](https://github.com/patchloom/patchloom/commit/b5ca6058a35879df6903578054aedd78b642f78e)), closes [#208](https://github.com/patchloom/patchloom/issues/208)
* diff summary line after preview output ([3952718](https://github.com/patchloom/patchloom/commit/3952718285deb778fd7b3e71f4275d596d214822)), closes [#359](https://github.com/patchloom/patchloom/issues/359)
* MCP benchmark 11/11 via anti-CLI instructions and diagnostic logging ([#478](https://github.com/patchloom/patchloom/issues/478)) ([d2e776d](https://github.com/patchloom/patchloom/commit/d2e776d4d2abe4b42fb60ec464bc91976db1ac59))
* **mcp:** add batch_replace and batch_tidy homogeneous batch tools ([#486](https://github.com/patchloom/patchloom/issues/486)) ([73981b4](https://github.com/patchloom/patchloom/commit/73981b4a588766be1399f258c46692a46553e6bd))
* op name aliases, consolidate doc_query, dynamic bench timeout ([#480](https://github.com/patchloom/patchloom/issues/480)) ([ffd0e3c](https://github.com/patchloom/patchloom/commit/ffd0e3c6fbf113b8ac712f32a4e3a2b0ac7a34cd))
* preserve TOML comments and formatting during doc operations ([3b245e3](https://github.com/patchloom/patchloom/commit/3b245e30960224fab5d8c6991268477e540daa5b)), closes [#202](https://github.com/patchloom/patchloom/issues/202)
* project config file (.patchloom.toml) for per-project defaults ([a02f71f](https://github.com/patchloom/patchloom/commit/a02f71fb1b051065803a127876d3f35098ab11ff)), closes [#355](https://github.com/patchloom/patchloom/issues/355)
* smart error recovery hints for no-match results ([fc3e7f3](https://github.com/patchloom/patchloom/commit/fc3e7f3510fd89c41c05708312621ac483a805f4)), closes [#357](https://github.com/patchloom/patchloom/issues/357)
* strengthen MCP agent-rules with tool selection guide ([#472](https://github.com/patchloom/patchloom/issues/472)) ([813d30f](https://github.com/patchloom/patchloom/commit/813d30f2862cc89371be795abe2d44eb9658a917))
* structured JSON APIs for batch and transaction MCP tools ([#473](https://github.com/patchloom/patchloom/issues/473)) ([84bed9f](https://github.com/patchloom/patchloom/commit/84bed9f89aa55c6e75c5c6143a4a9570e87b827b))
* tx search directory support, MCP lint-agents tool, example 08 smoke test ([6cf582b](https://github.com/patchloom/patchloom/commit/6cf582bf0fba079ecbf14b0a640d6110b8b6f32e))
* undo safety net with backup sessions ([4119e9a](https://github.com/patchloom/patchloom/commit/4119e9a02da3789aaf23f65db990b3836e98fd6a)), closes [#358](https://github.com/patchloom/patchloom/issues/358)


### Bug Fixes

* 4 Windows integration test failures ([3035e2a](https://github.com/patchloom/patchloom/commit/3035e2ae2e271c5d60a79faf3ba26f1df8feb24d))
* add backup support to delete and rename commands ([cc8e8c2](https://github.com/patchloom/patchloom/commit/cc8e8c29827d86c37d0275b87cab685c457724cc))
* add command prefix to md and read error messages ([6a5fd88](https://github.com/patchloom/patchloom/commit/6a5fd8869e6bf5e44866486c4c61db1bb072c368))
* add missing 'rename' to subcommand set, deduplicate driver helpers ([f0f7a37](https://github.com/patchloom/patchloom/commit/f0f7a37ca32ac87a95d6ab1bf7de3a30db34933f))
* add missing subcommands to agent driver subcommand set ([#444](https://github.com/patchloom/patchloom/issues/444)) ([4804b2d](https://github.com/patchloom/patchloom/commit/4804b2dbe1c76dd25eb8a55f52bead2927be855d))
* add test coverage and update install instructions for v0.1.0 ([#465](https://github.com/patchloom/patchloom/issues/465)) ([c42c7c1](https://github.com/patchloom/patchloom/commit/c42c7c1c2b726e95d64478f0c6656c79a5f46f18))
* add tilde fence support to lint-agents code block detection ([4ad6256](https://github.com/patchloom/patchloom/commit/4ad62563c31a094f9b80c1e656d8e6018292674f))
* address AI code quality findings in GrokDriver ([#453](https://github.com/patchloom/patchloom/issues/453)) ([1ecf4dc](https://github.com/patchloom/patchloom/commit/1ecf4dc79b45a1dbaa70e99df4c83bb3c0f8e1e5))
* agent bench file_ops collision and use focused agent-rules modes ([f86e02b](https://github.com/patchloom/patchloom/commit/f86e02b2ba1b3cab3eefe54e5453ce48c0432696))
* batch tokenizer silently drops empty quoted strings ([2da3d3f](https://github.com/patchloom/patchloom/commit/2da3d3ffa76c4607ab7ed86a4195d1e83dc2b398))
* bench CI replace uses wrong --from flag syntax ([cedaea0](https://github.com/patchloom/patchloom/commit/cedaea0691cc478d02bc30bfa8d043266a8dcc42)), closes [#343](https://github.com/patchloom/patchloom/issues/343)
* **bench:** prefer newest binary, add per-tool MCP log reporting ([#489](https://github.com/patchloom/patchloom/issues/489)) ([3868408](https://github.com/patchloom/patchloom/commit/3868408960a3385ca3ec28709c7e05d73f26ce99))
* **bench:** use neutral tidy prompt so agents discover batch_tidy ([#490](https://github.com/patchloom/patchloom/issues/490)) ([9fb5e90](https://github.com/patchloom/patchloom/commit/9fb5e90ad8276a65710adc2b92c9d249a355372a))
* **ci:** correct SBOM upload path for cargo-cyclonedx ([#462](https://github.com/patchloom/patchloom/issues/462)) ([07e7bd1](https://github.com/patchloom/patchloom/commit/07e7bd17054cb997f924efc43678c470d4b6149e))
* **ci:** disable fossa test until false positives are filtered ([#437](https://github.com/patchloom/patchloom/issues/437)) ([46e0e04](https://github.com/patchloom/patchloom/commit/46e0e04109f75cebd575117f2244b9169dae365e))
* **ci:** exclude securityscorecards.dev from lychee link checks ([#464](https://github.com/patchloom/patchloom/issues/464)) ([04ceac8](https://github.com/patchloom/patchloom/commit/04ceac8a98dee83ccabefc40bf879d5643c3e400))
* **ci:** make coverage badge step non-fatal when GIST_TOKEN missing ([#445](https://github.com/patchloom/patchloom/issues/445)) ([73c2e1e](https://github.com/patchloom/patchloom/commit/73c2e1e1b60ed141bbd0fff651bf0cdb93caa3de))
* **ci:** move FOSSA secret check from job-level to step-level ([#436](https://github.com/patchloom/patchloom/issues/436)) ([3f3be6f](https://github.com/patchloom/patchloom/commit/3f3be6fa1c19f35751f217877cb1cdf08b285ce4))
* **ci:** resolve Scorecard findings for token permissions and pinned deps ([#438](https://github.com/patchloom/patchloom/issues/438)) ([877f1fe](https://github.com/patchloom/patchloom/commit/877f1fe5c46738f7eef329436dfcab8b6e5f1a39))
* **ci:** upload Sigstore attestation bundles to GitHub Releases ([#466](https://github.com/patchloom/patchloom/issues/466)) ([2496409](https://github.com/patchloom/patchloom/commit/2496409382227576dcb997ed0c5a0c995b571f4d))
* colored diff output, edge-case tests, and clearer error messages ([#468](https://github.com/patchloom/patchloom/issues/468)) ([2eb5e39](https://github.com/patchloom/patchloom/commit/2eb5e394c0124446f7ae796ac59de4872cdebfee))
* complete driver refactoring (2 missed call sites, restore Path imports) ([7f389f8](https://github.com/patchloom/patchloom/commit/7f389f876df79cadca03bec00ea86077fb2d7cca))
* cross-platform backup paths for Windows drive letters ([334ecb4](https://github.com/patchloom/patchloom/commit/334ecb4a32db4e8bf21ced38c2e2f6acf6665062))
* doc append/prepend respect --quiet flag; add batch example ([3aade73](https://github.com/patchloom/patchloom/commit/3aade731c9d95abb8386796bf3073c543e78fc3c))
* **doc:** correct predicate syntax in doc select help text ([84471b9](https://github.com/patchloom/patchloom/commit/84471b96d5986a95a2d96d3f1c8532ce79d1b815))
* eliminate flaky validation tests caused by timestamp collision ([f112d97](https://github.com/patchloom/patchloom/commit/f112d97974e16797dec5cdc6ce7de608b61a807d))
* enable all integration tests on Windows ([74c4350](https://github.com/patchloom/patchloom/commit/74c4350ed0de45f2a0818b01240cee13c58dc9de)), closes [#218](https://github.com/patchloom/patchloom/issues/218)
* enable serde_json preserve_order to maintain JSON key ordering ([f5727a8](https://github.com/patchloom/patchloom/commit/f5727a826a97dd99cfc386d01039c6e51ce5955c))
* gate shell helpers with cfg(not(windows)) ([4c666ca](https://github.com/patchloom/patchloom/commit/4c666ca22a31d572e9c1dfcb1998b9799490bd58))
* improvement cycle 1 (create backup, tidy JSON, finalize ordering) ([#428](https://github.com/patchloom/patchloom/issues/428)) ([cabd164](https://github.com/patchloom/patchloom/commit/cabd164f38af7ab5f1ae5992edb0d98eca8cdc9e))
* improvement cycle 2 (delete backup, tidy exit code tests) ([#429](https://github.com/patchloom/patchloom/issues/429)) ([50f58c5](https://github.com/patchloom/patchloom/commit/50f58c5fa9bbedf1059d1ee493267d15631c600a))
* improvement cycle 3 (backup consistency, test coverage) ([#430](https://github.com/patchloom/patchloom/issues/430)) ([8486726](https://github.com/patchloom/patchloom/commit/8486726b4e4a05a8f23f97250fce3794863f5552))
* install lychee from GitHub releases on ubuntu-latest ([6c2d3bc](https://github.com/patchloom/patchloom/commit/6c2d3bc3361d2c44f3a923af4c417d6bd62f8a50))
* isolate Trivy from runner's broken Docker credential helper ([14734d2](https://github.com/patchloom/patchloom/commit/14734d26a81b5f1329f7fa5321d497d5ca7effc1))
* lint-agents skips dangerous commands inside code fences and inline code ([21ba873](https://github.com/patchloom/patchloom/commit/21ba873a633f5d1db40917865c1f34f225b44cc4))
* make update-readme portable across BSD and GNU sed ([3d60525](https://github.com/patchloom/patchloom/commit/3d60525ae2542aa8a276a84c598976842586e460)), closes [#360](https://github.com/patchloom/patchloom/issues/360)
* MCP batch test and enable MCP integration tests on all platforms ([ddff787](https://github.com/patchloom/patchloom/commit/ddff787670084014247965075dcbb3cc49a14a29))
* **mcp:** remove batch/transaction tools for zero-failure agent benchmarks ([#481](https://github.com/patchloom/patchloom/issues/481)) ([1ea3849](https://github.com/patchloom/patchloom/commit/1ea3849df96b0f11a110e3b17c8abfd66fcfaeba))
* md command errors now include the file path ([41b4533](https://github.com/patchloom/patchloom/commit/41b453390a635b9e11ead35349efe3e5a637a2b2))
* plan schema version, batch op limit, test hardening ([1c9b1a5](https://github.com/patchloom/patchloom/commit/1c9b1a573f02d1122fec210b94130fc843383b4e))
* preserve single-file text format in tx search, add path-prefix assertions ([54f420f](https://github.com/patchloom/patchloom/commit/54f420f5d30e259027b88590a08eecba74ea85b1))
* preserve YAML comments on array-resizing doc operations ([e8e1bfb](https://github.com/patchloom/patchloom/commit/e8e1bfbd7a71eb86fe4d980c0a055a01500c0c4b))
* prevent data corruption when backing up files outside project root ([be4bf78](https://github.com/patchloom/patchloom/commit/be4bf78606ab1281d427c7f1b411f46c7dda636e)), closes [#373](https://github.com/patchloom/patchloom/issues/373)
* propagate backup finalize errors instead of discarding them ([35d65f8](https://github.com/patchloom/patchloom/commit/35d65f8542f2e54c786ef22c0e20fe965d01b4e8))
* reject --nth 0 in replace and tx replace operations ([e025eaf](https://github.com/patchloom/patchloom/commit/e025eafd1369f51cf805b310654dc558f27f713b))
* reject invalid normalize_eol values and surface doc append/prepend errors ([b17bce8](https://github.com/patchloom/patchloom/commit/b17bce82759bc65627648175eb8dd79f4c9a05ae))
* remove unused import and write bench summary to step summary ([bed246c](https://github.com/patchloom/patchloom/commit/bed246cea8c4a289956810fef3a77f54a9090e54))
* rename command read_to_string now includes file path in errors ([9664b29](https://github.com/patchloom/patchloom/commit/9664b29c5b02d0ee652d9771090c3f2499d3fa1e))
* replace stale 'key' with 'selector' in all descriptions and docs ([ede2e1d](https://github.com/patchloom/patchloom/commit/ede2e1d903a8d463ce225f06194dd8c1134955b9))
* replace unwrap() with proper error in batch temp path ([fe2ed28](https://github.com/patchloom/patchloom/commit/fe2ed286f9eb4d4b2d886246ebc3c0f1613526f0))
* **replace:** include search path in no-match stderr message ([37f919a](https://github.com/patchloom/patchloom/commit/37f919a8b2254193b1d5872f9cfaf4514a5f4ec7))
* resolve 26 CodeQL Python quality findings ([#439](https://github.com/patchloom/patchloom/issues/439)) ([31a9d5d](https://github.com/patchloom/patchloom/commit/31a9d5d9745bae7b8027449ddc9e7951ff31ef89))
* resolve 5 open issues ([#409](https://github.com/patchloom/patchloom/issues/409)-[#413](https://github.com/patchloom/patchloom/issues/413)) ([75c8e82](https://github.com/patchloom/patchloom/commit/75c8e82afa9df5c38062571507cb1ee0ebdbdcb5)), closes [#410](https://github.com/patchloom/patchloom/issues/410) [#411](https://github.com/patchloom/patchloom/issues/411) [#412](https://github.com/patchloom/patchloom/issues/412)
* resolve cyclic imports and restore dynamic coverage badge ([#442](https://github.com/patchloom/patchloom/issues/442)) ([251287c](https://github.com/patchloom/patchloom/commit/251287c277f071af0eb8303de78901c511dc7cb2))
* resolve GitHub AI code quality findings ([#469](https://github.com/patchloom/patchloom/issues/469)) ([abfdbdb](https://github.com/patchloom/patchloom/commit/abfdbdbeec59cbb36ec8f5af5edb5497321a488d))
* resolve issues [#364](https://github.com/patchloom/patchloom/issues/364)-367 from Cycle 3 ([31e546b](https://github.com/patchloom/patchloom/commit/31e546b99cdb0ada383fbe976eea3afa6c067a87)), closes [#365](https://github.com/patchloom/patchloom/issues/365) [#366](https://github.com/patchloom/patchloom/issues/366) [#367](https://github.com/patchloom/patchloom/issues/367)
* resolve YAML merge keys (&lt;&lt;) during doc operations ([85f3e32](https://github.com/patchloom/patchloom/commit/85f3e32670b19fd50e015483c98fae62f97425f9)), closes [#203](https://github.com/patchloom/patchloom/issues/203)
* **selector:** reject ? prefix in predicate keys with helpful message ([723cae5](https://github.com/patchloom/patchloom/commit/723cae51a7c2fcdfc3e82435586476240fe9bf04)), closes [#403](https://github.com/patchloom/patchloom/issues/403)
* spec compliance fixes and test coverage for MPI Cycle 5 ([52ac410](https://github.com/patchloom/patchloom/commit/52ac410898fc08d74e44d24e442899f038bfdb62))
* undo correctly restores files that were outside the project root ([5d4b397](https://github.com/patchloom/patchloom/commit/5d4b397ae61b15008a38dbfa1a1d50892282e88c))
* update bench.yml upload-artifact to v7, add concurrency group ([4cbb7e9](https://github.com/patchloom/patchloom/commit/4cbb7e9cc62682af3fdb7dd88419013ef8dde52e))
* update stale test counts in README and agent test docs ([d57bc18](https://github.com/patchloom/patchloom/commit/d57bc182d70f7802442ea87886c93bbc22269afd))
* update-readme uses --all-features for accurate counts ([bf2f447](https://github.com/patchloom/patchloom/commit/bf2f447174a145471b6a52b77aeac66540fde64f))
* use .intoto.jsonl extension for attestation bundles ([#467](https://github.com/patchloom/patchloom/issues/467)) ([7c88e80](https://github.com/patchloom/patchloom/commit/7c88e80e3a8a452421a99e883e95f358af17f504))
* use correct lychee release tag and asset name ([05a6d72](https://github.com/patchloom/patchloom/commit/05a6d722626336d93ac65e50d540b538c942eddd))
* use ghcr.io for Trivy DB to avoid GCR credential errors ([78d7b74](https://github.com/patchloom/patchloom/commit/78d7b74ffae79900cb78698f327dd743a9b77bf6))
* use nanosecond timestamps in backup sessions ([5e1962a](https://github.com/patchloom/patchloom/commit/5e1962a5de0b814a73740039a4962689d91527e7)), closes [#363](https://github.com/patchloom/patchloom/issues/363)
* use streaming binary probe in tx dir search to avoid large allocations ([a05b639](https://github.com/patchloom/patchloom/commit/a05b639dd87ecbefd09efc22cebc79be3fcb14e0))
* warn on malformed .patchloom.toml, add backup pruning tests and troubleshooting docs ([2e300d5](https://github.com/patchloom/patchloom/commit/2e300d523c315a7b388d3828e16736b81defbfed)), closes [#369](https://github.com/patchloom/patchloom/issues/369) [#371](https://github.com/patchloom/patchloom/issues/371) [#372](https://github.com/patchloom/patchloom/issues/372)
* Windows backup test failures (directory open + external path prefix) ([be64e12](https://github.com/patchloom/patchloom/commit/be64e1288de5f10335568623add9fc1bb1fc441b))
* Windows CI test failures ([b32fa8d](https://github.com/patchloom/patchloom/commit/b32fa8dfcb92c3ef431689483d473e42c87f4471))
* Windows CI timeout in large-stderr validation test ([17efd15](https://github.com/patchloom/patchloom/commit/17efd157649c8d8faa3326151234b112cbf28999))
* Windows path colon in files_with_matches test ([8722360](https://github.com/patchloom/patchloom/commit/87223601d1381afbe98958d9d3a59774617678a5))
* wire prune_old_backups into backup session creation ([2709ce7](https://github.com/patchloom/patchloom/commit/2709ce7861c072850db1f08f759251f2853cad71))
* YAML CST safety net for array length changes, add parse validity checks ([24e4de0](https://github.com/patchloom/patchloom/commit/24e4de021e020b3da7fc1c2ab9bee8889a566819)), closes [#209](https://github.com/patchloom/patchloom/issues/209) [#210](https://github.com/patchloom/patchloom/issues/210) [#211](https://github.com/patchloom/patchloom/issues/211)


### Performance Improvements

* cache canonicalized cwd in MCP server ([c0ef850](https://github.com/patchloom/patchloom/commit/c0ef850c9acf50fbea233c8d0b89218680918674))
* cache parsed serde_json::Value in tx to avoid redundant parse-serialize cycles ([7812df2](https://github.com/patchloom/patchloom/commit/7812df247ef8468255f161090e92dfaa648c3e6b)), closes [#250](https://github.com/patchloom/patchloom/issues/250)
* four targeted optimizations across hot paths ([3d0ab90](https://github.com/patchloom/patchloom/commit/3d0ab901b70d3704036c79cc551d6dcbfe100bf8))
* reduce allocations across replace, search, doc ops, diff, and selector ([5543443](https://github.com/patchloom/patchloom/commit/5543443a25147a9df4d37c8715eb0639648045d3))
* trim agent_rules.md from 102 to 40 lines (71% smaller) ([cecbc18](https://github.com/patchloom/patchloom/commit/cecbc18d08373b8169d1e9be1a76a28e716b215e))
* use parallel file walking for directory traversal ([4a68a85](https://github.com/patchloom/patchloom/commit/4a68a85347151d5961078cc2548a1782f291ebe6)), closes [#249](https://github.com/patchloom/patchloom/issues/249)

## [0.1.0] - 2026-06-04

### Security

- Fixed external path traversal bypass in `undo --apply` restore logic: crafted `__external__/../..` manifest entries could overwrite files outside the project root
- Added syntactic path traversal validation to undo restore paths
- Added `validate_path_resolved` symlink check to all 16 MCP write handlers

### Commands

19 commands (including `mcp-server`, enabled by default) covering search, structured editing, batching, and file operations:

- **search** / **replace** - Literal and regex search/replace across files, with context lines, `--nth`, `--case-insensitive`, `--insert-before`/`--insert-after`, `--assert-count`, and `--if-exists` for idempotent runs
- **doc** - Parser-backed JSON, YAML, and TOML editing (get, set, delete, merge, append, prepend, update, move, ensure, delete-where, select, flatten, diff). Preserves comments and formatting in YAML and TOML
- **md** - Heading-aware markdown editing (replace-section, insert-after/before-heading, upsert-bullet, table-append, dedupe-headings, lint-agents)
- **tx** - Atomic multi-file transactions with 23 operation types, format/validate lifecycle, strict rollback mode, and YAML/TOML plan format support
- **batch** - Line-oriented multi-operation syntax for quick multi-file edits without JSON
- **patch** - Apply or check unified diffs with fuzz matching
- **create** / **delete** / **rename** - File lifecycle operations with `--apply`/`--check`/`--force` modes. Rename handles binary files natively via `fs::rename`
- **read** / **status** - File inspection and git working-tree status
- **mcp-server** - MCP protocol server exposing all operations as structured tool calls
- **agent-rules** / **completions** - Generate AI agent instructions or shell completions

### Structured file safety

- YAML and TOML edits preserve inline comments, section comments, and formatting (CST-level editing)
- JSON/YAML/TOML mutations are parser-backed; output is always valid
- Sequence-rooted YAML files are handled correctly (falls back to non-preserving serialization when root is not a mapping)
- `doc` operations include depth guard (128 levels) on deep merge to prevent stack overflow
- All file writes go through atomic write (tempfile + rename) for crash safety

### Batching and transactions

- `tx` plans support `format` and `validate` lifecycle arrays with configurable timeouts
- `strict` mode reverts all writes on format/validate failure (exit code 7)
- `read` and `search` operations in tx plans for inspect-then-edit workflows in a single call
- `batch` provides simpler line-oriented syntax covering 20 operation types
- Operation ordering is well-defined: last write wins, delete-then-create works, each op sees prior results
- CLI `tx` validates plan `cwd` is a directory, returning PARSE_ERROR instead of confusing OS errors
- Relative plan `cwd` values resolve from invocation root, matching MCP behavior
- Lifecycle shell commands (format/validate) now capture first 512 bytes of stderr in error output
- Lifecycle failure messages include the working directory (`cwd: .` or `cwd: nested`)

### Correctness fixes

- `file.create` after `file.delete` in the same tx plan no longer silently loses the file
- Empty `--from` in replace/tx is rejected instead of inserting between every character
- tx replace with conflicting fields (`to` + `insert_before`) returns PARSE_ERROR
- tx replace missing all output fields returns PARSE_ERROR instead of silently deleting
- Replace-only tx plans with zero matches return NO_MATCHES (exit 3) instead of SUCCESS
- tx glob replace no longer buffers non-matching files into pending state
- `create --check` verifies parent directory exists (non-force mode)
- Race-free file creation via `File::create_new` for `create --apply` and tx `file.create`
- Fixed `read_file_content` double-join bug when transaction cwd is relative
- `create` command: backup finalize was called before the atomic write, producing a backup for a change that had not yet happened; finalize now runs after the write succeeds
- `create` command now creates backup sessions before writing, enabling `undo --apply` to remove or restore files created with `create --apply`

### Output and diagnostics

- `--json` structured output on all commands including tx error paths
- `--jsonl` streaming output for search and read
- Explicit `error_kind` values in tx JSON output (parse_error, rollback, validation_failed, format_failed)
- Stderr diagnostics for silently skipped files in search, replace, and tx glob
- File path context in doc operation error messages
- Improved doc command error messages to list supported file extensions
- `tidy fix` now emits structured JSON/JSONL output when `--json` or `--jsonl` is active, matching other write commands

### MCP server

- MCP `search_files` tool exposes `invert_match` and `assert_count` parameters, matching CLI and tx feature parity
- MCP `search_files`, `replace_text`, and `fix_whitespace` tool descriptions document text-file semantics (binary and invalid UTF-8 files are skipped)
- MCP `transaction` validates relative `cwd` resolves to a directory, not a file
- Cached canonicalized cwd at startup, eliminating redundant `realpath` syscall per tool invocation
- Consolidated `validate_path_contained` + `validate_path_resolved` into single `check_path` method, preventing partial validation
- Shared `resolve_plan_cwd` function deduplicates CLI and MCP cwd resolution

### Testing and benchmarks

- 1100+ tests verified on Grok 4.3, GPT-5.4, and Claude Opus 4.6
- Agent integration tests: 19 scenarios with invocation-capture shim
- 5 fuzz targets: selector parse, patch parse, patch apply, batch tokenize, selector eval
- CLI benchmarks vs native tools (grep, sed, jq) using hyperfine
- Agent A/B benchmarks measuring duration, tool calls, and success rate

### Internal improvements

- Extracted shared tx execution core (`execute_and_collect`, `run_lifecycle`) eliminating ~190 lines of duplication
- Extracted `backup_write_files` helper, refactored 5 call sites across replace, patch, and tidy commands
- Extracted `apply_replacements` helper in replace command, deduplicating backup+write block
- Extracted `with_doc_mutation` helper in doc command, eliminating 9x load/clone/serialize/write boilerplate
- Extracted `compile_replace_regex` shared helper

### Infrastructure

- MSRV: Rust 1.95+
- License: MIT OR Apache-2.0
- CI: fmt, clippy, tests, MSRV check, dependency audit, doc freshness checks, code coverage, Codecov upload
- CI: benchmark summary table with 90-day artifact retention and cross-run regression detection (20% threshold, 2ms minimum)
- `make check` runs the full gate locally, including generated doc freshness

### Documentation

- Documented column offset semantics in search JSON output
- Added `init` command to README Commands table
- Documented stderr capture and cwd context in lifecycle failure output (reference docs, quickstart)
- Added `cargo check --all-targets` to CONTRIBUTING.md for default-feature build verification

[Unreleased]: https://github.com/patchloom/patchloom/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/patchloom/patchloom/releases/tag/v0.1.0
