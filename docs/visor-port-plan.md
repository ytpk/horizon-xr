# План порта Visor в Horizon

Дата анализа: 2026-11-06. Анализ вёлся на ветке `vivecraft-port`; реализация идёт на ветке `visor-port` (статусы фаз в конце раздела 9, подробности и чеклист для шлема в `docs/visor-port.md`). Первоначальный текст был только анализом; места, где Java-код (c01354f, b4f3fc8) разошёлся с планом, исправлены при реализации и помечены **(исправлено)**.

## 0. Что нужно знать сразу

### 0.1. Папка `VivecraftMod-26.3-1.3.18` это Vivecraft, а не Visor

README, `gradle.properties` и `LICENSE.md` в `<workspace>/VivecraftMod-26.3-1.3.18` описывают **Vivecraft 1.3.18 для Minecraft 26.3** (`mod_id=vivecraft`, `minecraft_version=26.3`, `mod_version=1.3.18`, лицензия LGPLv3, авторы fayer3, Ferri_Arnus, IMS, jrbudda, minecraft player, Techjar). Это не Visor. У папки нет `.git`, поэтому коммит апстрима неизвестен (в THIRD_PARTY_NOTICES придётся указывать версию 1.3.18, а не хеш, либо взять хеш релиза-тега 1.3.18 из GitHub Vivecraft/VivecraftMod, не проверено).

Использую папку как эталон Vivecraft для MC 26.3. Что изменилось относительно 1.3.15 (см. 2.3): протокол вырос с версии 11 до 12 (новый пакет `ROOMSCALE_ATTACK`), остальное это перестройка рендера под 26.3 и мелочи.

### 0.2. Visor найден, лицензия MPL-2.0, вопрос с политикой закрыт

- Visor: https://github.com/VisorModStudio/Visor (форки UrBoiTom/Visor и LPlusBonk/Visor не официальные). Лицензия на HEAD: **Mozilla Public License 2.0** (`LICENSE.md`, 373 строки, текст MPL 2.0, Exhibit A, без Exhibit B то есть не "Incompatible With Secondary Licenses"). `RELICENSING.md`: в августе 2026 проект перешёл с LGPL-3.0 на MPL-2.0, не ретроактивно; релизы до смены остаются LGPL-3.0; согласия трёх авторов собраны 2026-08-10. `CONTRIBUTING.md`: вклад лицензируется MPL-2.0, плюс право Phoenix-Ra перелицензировать в будущем.
- Клон: `<workspace>/Visor/src`, коммит `c01354fad8ac03a016c249239a04b07adeb36476` ("version 0.6.1", 2026-11-01). Клон серверного плагина: `<workspace>/Visor/plugin`, репозиторий https://github.com/VisorModStudio/VisorPlugin, коммит `b4f3fc8be0e67f517c3e98c07ac1b5510eaf3d24` ("version 0.2.0alpha", 2026-08-11), лицензия MPL-2.0 (`LICENCE.md`).
- Другие страницы: Modrinth https://modrinth.com/mod/visor (MPL-2.0, Fabric/Forge/NeoForge/Quilt, версии MC 26.3, 26.2, 26.1.x, 1.21.x, 1.20.x), wiki https://github.com/VisorModStudio/Visor/wiki (неполная), релизы https://github.com/VisorModStudio/Visor/releases, Discord https://discord.gg/wJX8sTDEdx.

**Решение (принято пользователем): конфликт с политикой проекта снят.** Это личный форк, он не вливается в репозиторий Horizon и будет опубликован для сообщества; разрешение обеих сторон (Horizon и авторов Visor) получено. Читать и портировать код Visor можно, фаза 0 не нужна, вопросы разработчикам Visor (раздел 11) работу не блокируют. `CONTRIBUTING.md` не менялся. Минимальные пометки сделаны, потому что форк публичный: крейт `crates/horizon-visor` (`license = "MPL-2.0"`, `LICENSE-MPL-2.0`), SPDX-заголовок и авторы/коммит-источник в файлах с портированным кодом, раздел "Visor" в `THIRD_PARTY_NOTICES.md`, строки на экране credits.

### 0.3. Три проверенных вывода

1. Протокол Visor **не совместим** с протоколом Vivecraft: другой канал, другая схема версии, другой набор пакетов (см. раздел 2). Vivecraft-клиент не говорит с Visor-сервером и наоборот. Для "полной совместимости на серверах" Horizon должен уметь **оба** протокола и выбирать по тому, какой канал есть на сервере.
2. Большая часть Visor (около 60% строк Java) завязана на Minecraft: миксины, GUI-виджеты поверх `Screen`, рендер-таргеты, совместимость с Sodium/Iris/Create. На Horizon (нативный Rust, wgpu) это не переносится, а заменяется существующими слоями.
3. Утверждение "Visor быстрее" я в коде не подтвердил (см. 3.4). Рендер у Visor, как у Vivecraft, отдельный проход на каждый глаз с обновлением видимости на каждый проход.

## 1. Что такое Visor

"Next-Gen VR core mod for Minecraft. Based on OpenXR. Built as a platform. Supports addons" (описание репозитория и Modrinth). Java, Gradle + Stonecutter (одна кодовая база на 1.20.1 до 26.3), модули `visor-api`, `visor-core`, `visor-fabric`, `visor-forge`, `visor-neoforge`. Версия 0.6.1, статус beta ("может значительно измениться несколько раз до стабильной версии"). Организация также ведёт VisorPlugin (Spigot/Paper/Folia, alpha, "будет переписан осенью 2026"), VisorEssentials (аддон), шаблон аддона.

Размер (строки Java, `wc -l`, коммит c01354f): всего 106 023; `visor-api` 32 586; `visor-core` 71 152; загрузчики 2 285. VisorPlugin 9 778 (из них около 3 500 это почти одинаковые адаптеры NMS под версии).

Внешняя зависимость: **AtumVR** (`libs/atumvr-{api,core}-1.14.0beta-snapshot#25..27.jar`, бинарники в репозитории) от того же автора, Apache-2.0, исходники публичны: https://github.com/Phoenix-Ra/AtumVR. Это слой OpenXR (LWJGL), ввод, профили контроллеров, трекеры рук/тела, хаптика (bHaptics), тредмилы (KAT, Infinadeck). Его исходники и jar я не читал, только список пакетов. Значительная часть "чистоты" Visor живёт там, а не в репозитории Visor.

## 2. Сеть и совместимость

### 2.1. Сравнение протоколов

| | Vivecraft (1.3.15 и 1.3.18) | Visor 0.6.1 / VisorPlugin 0.2.0alpha |
|---|---|---|
| Канал | `vivecraft:data` | `visor:channel` (`VisorNetwork.CORE_CHANNEL_ID`) |
| Версия | enum `NetworkVersion`, на проводе `ordinal - 1`; 1.3.15: 11 (`CLIMBEY_JUMP`), 1.3.18: 12 (`ROOMSCALE_ATTACK_PACKET`); сервер и клиент согласуют минимум, старые клиенты работают | единственный `int CORE_NETWORK_VERSION = 5` ("since Visor 0.5.0"); **при несовпадении сервер отключает игрока** (`visor.messages.network_mismatch`), в плагине то же |
| Идентификатор пакета | байт = ordinal `PayloadIdentifier` (отдельные enum для C2S и S2C) | байт = ordinal `VisorCorePayloadID` (один общий enum из **27** значений (**исправлено**: в плане было 28): HANDSHAKE 0, ROTATION_Y 1, OFFHAND_SLOT 2, SERVER_SETTINGS 3, BLOCK_DAMAGE 4, OTHER_VR_START_TRACKING 5, OTHER_VR_POSE_DATA 6, OTHER_VR_ROTATION_Y 7, OTHER_VR_LEFT_HANDED 8, OTHER_VR_BODY_TYPE 9, OTHER_VR_WORLD_SCALE 10, OTHER_GUN_ANGLE 11, OTHER_VR_FULL_HEIGHT 12, FULL_HEIGHT 13, POSE_DATA 14, VR_BODY_TYPE 15, LEFT_HANDED 16, ACTIVE_HAND 17, WORLD_SCALE 18, GUN_ANGLE 19, CRAWLING 20, CLIMBING 21, TELEPORT 22, SWING_ATTACK 23, SWING_BLOCK 24, OVERLAY_FOCUSED 25, OTHER_VR_OVERLAY_FOCUSED 26) |
| Рукопожатие | клиент шлёт `VERSION` (строка версии мода + флаги, `NetworkVersion`), сервер отвечает набором S2C пакетов (версия, настройки, права, телепорт, "VR changes" и т.д.) | клиент шлёт `HANDSHAKE` **в конце `handleLogin` на любой сервер, не зная, умеет ли он Visor** (`bool vrActive, int32 networkVersion, rest-of-buffer UTF-8 visorVersion`, версия строкой мода, у Horizon `0.6.1`); сервер отвечает пустым `HANDSHAKE`, затем `SERVER_SETTINGS`, затем (только VR) `ROTATION_Y` и `OFFHAND_SLOT` (если `twoHandedVR`); клиент считает сервер Visor-сервером только после этого ответа |
| Поза игрока (C2S) | отдельные пакеты `UBERPACKET`-типа: head/controller0/controller1 и т.п., плюс FBT в версии `FBT` | один `POSE_DATA`: HMD, главная рука, вторая рука (каждый `vec3 float32 x3` + `quat float32 x4` в порядке **x, y, z, w** (у Vivecraft w, x, y, z), **относительно позиции игрока (ног)**, ориентация в мировых координатах), затем трекеры тела (VarInt-битмаска на 14 элементов: waist, chest, ноги, лодыжки, колени, запястья, локти, плечи), затем суставы кистей (две руки, VarInt-битмаска `VRHandJointType.COUNT`, элемент как выше) |
| Другие игроки | S2C с позами всех VR-игроков (UberPacket) | `OTHER_VR_START_TRACKING` (UUID + снимок всех полей), затем `OTHER_VR_POSE_DATA` каждый тик по отслеживаемым сущностям, и отдельные `OTHER_VR_BODY_TYPE/LEFT_HANDED/ROTATION_Y/WORLD_SCALE/FULL_HEIGHT/GUN_ANGLE/OVERLAY_FOCUSED` только по изменению |
| Серверные настройки | набор типизированных S2C-пакетов | один текстовый пакет `SERVER_SETTINGS`: плоский YAML, ключи - **имена Java-полей** `VRServerSettings` (`twoHandedVR`, `betterSwinging`, `roomscaleShieldBlocking`, `attacksWhileBlocking`, `roomCrawlingSupported`, `roomClimbingSupported`, `supportedMovement`, `supportedHeightMode`, `teleportUp/Down/ForwardLimit`, `pvpVRvsVanilla`, `pvpVRvsVR`, `bodyTrackersSupported`, `handTrackersSupported`, ...; **исправлено**: `two_handed` и т.п. это ключи экрана настроек клиента, не провода). Плагин `supportedHeightMode` не шлёт |
| Игровые пакеты | CLIMBEY_JUMP, HAPTIC, DAMAGE_DIRECTION, AIM_*_OVERRIDE, DUAL_WIELDING, ROOMSCALE_ATTACK (1.3.18) и др. | SWING_ATTACK (`int entityId, bool shift, bool mainHand`), SWING_BLOCK, TELEPORT (`float x,y,z`), CRAWLING, CLIMBING, BLOCK_DAMAGE (`UUID, blockPos, int stage`) |
| Совместное существование | - | Visor при загрузке обнаруживает мод `vivecraft` и просит убрать один из них (`VisorState`); "как два VR-ядра конфликтуют" (Modrinth, сторонний листинг) |

Формат всех пакетов прочитан из `visor-api/.../common/network/**` (коммит c01354f) и подтверждён в `VisorPlugin/visor-api/.../VisorNetwork.java` (тот же канал, версия 5). Явного байт-в-байт тестового набора в репозитории я не искал (неподтверждено, есть ли такие тесты).

### 2.2. Ответы на вопросы задачи

- **Тот же ли протокол, что у Vivecraft?** Нет. Только "общий корень" идей (пакет позы, настройки сервера, телепорт).
- **Может ли Vivecraft-сервер/плагин говорить с Visor-клиентом?** Нет: Visor-клиент шлёт на `visor:channel`, которого у Vivecraft-сервера нет (поведение сервера на неизвестный канал: вероятно игнор, не проверено).
- **Может ли Visor-сервер говорить с Vivecraft-клиентом?** Нет, по той же причине. Такой клиент остаётся для сервера обычным ванильным игроком (на Visor-сервере с `vrOnly` его отключит через 5 секунд, кроме операторов, `ServerNetworking.kickDelayedIfNoVR`).
- **Есть ли собственный серверный плагин?** Да. Для Fabric/Forge/NeoForge серверная часть это тот же мод (`visor-core`, раздел `core/server`, 1 534 строки Java, плюс общие миксины `mixin/common`). Для Spigot/Paper/Folia это VisorPlugin 0.2.0alpha (Bukkit, NMS-адаптеры 1.20.1 ... 26.2; BungeeCord/Velocity в планах). Плагин поддерживает ViaVersion. Команды `/visor reload|status|trackers|hands`, право `visor.admin`.
- **Какие протоколы должен говорить Horizon?** **Оба**, один активный на соединение, выбор по присутствию канала на сервере (регистрация каналов в конфигурационной фазе; запасной вариант: сначала проба по бренду сервера/ответу). Vivecraft покрывает уже существующие серверы Paper+Vivecraft и Fabric, Visor покрывает серверы с VisorPlugin или модом Visor. Один из двух только по выбору пользователя не даёт "полной совместимости".

### 2.3. Разница Vivecraft 1.3.15 -> 1.3.18 (снимок для порта)

`diff -r` по `common/src`: 143 файла отличаются, около 3 500 изменённых строк из примерно 69 000 (почти всё это адаптация рендера к 26.3: `FirstPersonHandsAndItems*`, удалены `CubeMapVRMixin`, `ItemInHandRendererVRMixin`, `LevelTargetBundle*`, добавлен `GlRenderPipelineVRMixin` в OptiFine-совместимости; переименование `EnderMan` -> `Enderman`). **Сетевые изменения единственные:**

1. `NetworkVersion.ROOMSCALE_ATTACK_PACKET` (новая версия 12).
2. `PayloadIdentifier.ROOMSCALE_ATTACK` (C2S и S2C).
3. C2S `RoomscaleAttackPayloadC2S(boolean isRoomscaleAttack, byte hitsMade)`: байт id, bool, byte. S2C `RoomscaleAttackPayloadS2C()`: только id; сервер шлёт его, если `ROOMSCALE_ATTACK_PACKET.accepts(networkVersion)`, клиент выставляет флаг `SERVER_SUPPORTS_ROOMSCALE_ATTACK_PACKET` и шлёт `(true, 0)` при начале удара по блоку (`SwingTracker`) и `(false, totalHits)` при его окончании.
4. Серверная часть (`ServerNetworking`, `ServerPlayerGameModeMixin`, `ServerPlayerGameModeAccessor`): `isHitRoomscale` и накопление `roomscaleHitProgress += hitsMade * destroyProgress`, чтобы удары комнатного масштаба ломали блок с правильной скоростью.

В текущем порте Horizon: `crates/horizon-vivecraft/src/network/version.rs` перечисляет 11 значений до `ClimbeyJump`, так что порт на 1.3.18 это добавить `RoomscaleAttackPacket` (+12), два пакета (около 60 строк) и вызов в логике удара (около 80 строк), с тестами порядка 100 строк. Рендер-часть 1.3.18 Horizon не касается.

## 3. Архитектура: Visor против Vivecraft

### 3.1. Модули

| Область | Vivecraft (1.3.15) | Visor 0.6.1 |
|---|---|---|
| Раскладка | один `common` + loader-проекты, `api`/`api_beta`, миксины в `mixin/client_vr`, `mod_compat_vr` | `visor-api` (публичный API, 32.6K), `visor-core` (реализация, 71K), три загрузчика; всё как `Visor*` интерфейсы + `*Impl` |
| API | небольшой (`api`, `api_beta`) | широкий: события (`api/client/events`), игроки (`VRLocalPlayer`, `VRRemotePlayer`, позы), ввод (`VRInputManager`, действия, наборы), GUI/оверлеи, настройки, аддоны (`VisorAddon`, `VisorChannel` для собственных каналов аддонов с `networkVersion`) |
| OpenXR | собственный JNI/LWJGL слой внутри мода (SteamVR/OpenVR исторически, OpenXR добавлен) | вынесен в **AtumVR** (отдельная библиотека), `provider/openxr` 1.5K строк обвязки: `XrProvider`, `XrInputHandler`, `XrRawPoseHandler`, `XrRenderer` |
| Ввод | `ControllerInput`, привязки KeyMapping, SteamVR-действия | `VRInputManagerImpl`, наборы действий (`ActionSetRegistry`), игровые действия (`GameActionMovement/Jump/Rotate/Sprint/HotBar`), `TreadmillInput`, редирект мыши |
| Трекеры | `client_vr/gameplay/trackers/*` (Swing, Teleport, Climb, Bow, Row, Vehicle, Backpack ...) | `core/client/tasks/types` (TaskSwing, TaskHotBar, TaskRoomConsume, movement: Climb, Crawl, Jump, Sneak, Swim, Teleport, vehicle) - 3.2K |
| Поза и модель | `VRData`, `VRPlayer`, `VRPlayerModel` | `player/pose` (2.2K, `raw` слои, TICK/RENDER позы), `body` (full body, `VRBodyType`), `height`, `VRRemotePlayerImpl`, рендер `VRPlayerRendererFull`/`HandsOnly` |
| GUI | экраны Vivecraft + GuiHandler (экран на плоскости) | собственная система оверлеев (`gui/overlays`, 6.6K) и экранов (`gui/screens`, 9.3K) + 15K API виджетов; настраиваемые пользователем оверлеи, раскладки, клавиатура (7 файлов), пресеты (`assets/visor/presets`) |
| Настройки | `VRSettings` + options table | `VRClientSettings` (статика), `settings/options`, `presets`, категории экранов настроек (render, controls ...) |
| Рендер | проходы глаз/центр/третье лицо, `RenderPassManager`, свои таргеты | `VRRendererBase` (469), `VRRenderState` (фазы: vanilla/GUI/VR-проход), `MultiCameraRenderTarget` (подмена цели по проходу), `RenderTargetMain/First/Third/GUI`, `render/decoration` (6.5K: рука, эффекты, главное меню), шейдеры `vr_*` |
| Совместимость | Sodium, Iris, OptiFine и др. | Sodium, Iris, Xaero, Create/Aeronautics, Voxy, Flashback/ReplayMod, DH, Blur ... (около 6.4K core + 6.6K API) |
| Серверная часть | `server/*`, `ServerNetworking`, `ServerVivePlayer`, миксины | `core/server` (1.5K), `ServerHandsPose`/`ServerTrackersPose`, `VRServerSettings` (аннотации `@SendSettingToClient`, `@VRServerOptionField`, комментарии конфига) |

### 3.2. Что в Visor лучше с точки зрения структуры

- Явные границы: `api` (контракты) против `core` (реализация); события вместо прямых вызовов; аддоны с собственными сетевыми каналами (`VisorChannel`) и версиями.
- Данные позы сериализуются через небольшие `*Buffer` записи (`PoseDataBuffer`, `PoseElementBuffer`, `PoseHandsBuffer`, `PoseTrackersBuffer`), с битмасками отсутствующих элементов. Для Horizon это удобная модель.
- Руки (суставы) и FBT (14 точек) в протоколе сразу, а у Vivecraft FBT добавлен позже и только часть.
- Настройки сервера в одном текстовом пакете: добавить параметр можно без новой версии сети.
- Меньше легаси: нет SteamVR/OpenVR-специфики в ядре.

### 3.3. Что остаётся слабым / неизвестным

- Протокол молод и нестабилен: версия 5 с 0.5.0, README: "can be changed significantly couple of times", плагин будет переписан. Любой порт привязан к движущейся цели.
- Несовместимость версий жёсткая (кик), а не согласование минимума как у Vivecraft.
- Документация (wiki) неполная.
- Нет подтверждённых тестов формата пакетов в репозитории (не искал целенаправленно).

### 3.4. Почему "быстрее" (проверка утверждения)

Прочитано: `VRRendererBase`, `VRRenderState`, `MultiCameraRenderTarget`, `LevelRendererCullMixin`, `LevelExtractorMixin`, список миксинов рендера. Findings:

- Структура кадра классическая: проходы EYE_LEFT и EYE_RIGHT (+ CENTER/THIRD_PERSON для зеркала), каждый проход заново экстрагирует и рисует уровень. В `LevelRendererCullMixin` явно сказано "each VR pass has its own camera, visibility cached by the previous pass won't work" и граф окклюзии инвалидируется на каждый проход (кроме случая с Sodium, где этим занимается Sodium). Это не экономия, а то же поведение, что у Vivecraft.
- Есть: маска скрытой области (`setupHiddenArea`, "stencil mask"), масштаб рендера с "ожиданием остановки слайдера" перед пересозданием таргетов, зеркало может быть выключено (`MirrorMode.OFF`), таргеты третьего лица и центра создаются только если нужны, `ClientFeaturesToggle` включает функции по событиям.
- Я не нашёл ни multiview, ни повторного использования данных экстракции между глазами, ни поздней синхронизации позы. Вероятные причины ощущения скорости (не проверено): современные миксины вместо патчей, опора на Sodium, меньше работы на тик (задачи включаются по необходимости), AtumVR. Просить у разработчиков Visor бенчмарки и конкретные места (раздел 9).

### 3.5. Идеи для CPU-bound кадра на Quest (90 Гц держит, цель 120 Гц)

Из самого Visor (что подтверждено кодом):

1. Маска скрытой области (XR_KHR_visibility_mask): не рисовать невидимые пиксели. Выигрыш в основном по GPU; для CPU нулевой.
2. Динамический масштаб рендера с гистерезисом (не пересоздавать цели при движении слайдера).
3. Не создавать и не обновлять зеркало и третье лицо на Quest (у Horizon их и так нет).
4. Включение задач и функций по условию, а не всегда (`ClientFeaturesToggle`): для Horizon значит, что трекеры тела и рук работают, только если сервер/настройки их разрешают.

Не из Visor, но важнее для CPU (предложения, нужны замеры; не проверено на устройстве):

5. **Single-pass multiview** (`VK_KHR_multiview` через wgpu) вместо двух проходов: примерно вдвое меньше CPU на сабмит draw-вызовов. Ни Visor, ни Vivecraft этого не делают, Horizon единственный, у кого это возможно из-за собственного рендера.
6. Общая подготовка кадра для обоих глаз: culling/сортировка один раз по объединённому фрустуму, на глаз только матрицы (в Visor специально отключено из-за Java-рендера Minecraft; в Horizon это под контролем автора).
7. **Application SpaceWarp** (XR_FB_space_warp) и/или foveated rendering (XR_FB_foveation): держать 60 Гц рендера при 120 Гц показа. Это самый короткий путь к 120 Гц при CPU-bound кадре.
8. Рассылка позы по сети не чаще, чем нужно, и пропуск неизменившихся полей (как в Visor `*LastSent`), а также снимок `OTHER_VR_START_TRACKING` вместо повторов: экономит CPU на сервере и клиенте.

## 4. Инвентаризация текущего порта Horizon

Строки Rust (`wc -l`, ветка `vivecraft-port`, HEAD `90fd56df6`; 30 коммитов относительно `quest-openxr`; всего в `crates` около 1.14M строк):

| Область | Файлы | Строки |
|---|---|---|
| `crates/horizon-vivecraft` (LGPL) | см. ниже | **15 335** |
| из них сеть (`network/*`, `wire.rs`) | s2c 466, c2s 425, pose 214, body 92, version 89, mod 105, wire 165 | 1 556 |
| из них сервер (`server.rs`) | | 1 301 |
| из них таблицы опций (`options_table.rs` 1 457, `settings_table.rs` 1 176, `settings.rs` 506, `options.rs` 295) | | 3 434 |
| из них трекеры (`trackers/*`: climb 917, swing 587, teleport 578, hands 530, scope 371, vehicle 286, body 256, moves 226, mod 42) | | 3 793 |
| из них игрок/модель/данные (`player.rs` 861, `player_model.rs` 820, `vr_data.rs` 495, `math.rs` 243, `client.rs` 535) | | 2 954 |
| из них клавиатура, GUI, страницы, радиальное меню и прочее (`keyboard.rs` 715, `gui.rs` 485, `pages.rs` 400, `crosshair.rs` 167, `radial.rs` 156, `overrides.rs` 155, `modes.rs` 155, `lib.rs` 64) | | 2 297 |
| `horizon-client/src/game/vr*.rs`, `vivecraft_*.rs`, `headset*.rs` | vr.rs 1 538, headset.rs 770, headset_hands.rs 468, vivecraft_server.rs 414, vr_keyboard.rs 286, vivecraft_net.rs 276, headset_radial.rs 275, headset_trackers.rs 205, headset_bow.rs 173 | **4 405** |
| `horizon-xr` (LGPL) | lib.rs 1 611, actions.rs 266, panel.rs 126 | **2 003** |
| `horizon-quest` (MIT) | lib.rs 207, logcat.rs 63 | 270 |
| `horizon-client-input/src/headset.rs` (MIT) | | 521 |
| `horizon-client-render/src/vr_options.rs` | | 81 |
| `horizon-client-render/src/options_ui.rs` (весь UI опций Horizon, VR-страницы лишь часть) | | 8 655 |
| **Итого VR-ядро (без `options_ui.rs`)** | | **22 615** |

Остальной клей (`mobs/anim.rs`, `shield.rs`, `creatures.rs`, `boats.rs`, `remote_vehicles.rs`, `lan.rs`, `lan_actions.rs`, `net.rs`, `native_net.rs`, `title.rs`, `control.rs` и др.) считать построчно я не стал (неподтверждено; это десятки-сотни строк на файл).

Статус по `docs/vivecraft-port.md`: четыре раунда, почти всё "ported-untested" (нет устройства), юнит-тесты `horizon-vivecraft` (134), `options UI` (68). Не портировано: телескоп, прозрачность руки/предмета, меню-комната, GuiKeyboard-указка, экран привязок, CrawlTracker и CameraTracker не подключены, третье лицо.

Всего по Vivecraft 1.3.15 (Java `common/src`): 68 466 строк, по 1.3.18: 69 058.

## 5. Совместимость: матрица

Что Horizon должен реализовать для каждой пары "клиент/сервер":

| Горизонт-клиент подключается к... | Нужно | Сейчас | После порта |
|---|---|---|---|
| Ванильный сервер | настройки по умолчанию для "не-VR-сервера" (в Visor `joinedDedicatedServer()`: ползучесть/лазанье выключены, движение CONTROLLER, режим роста MATCH_MODEL) | есть в логике Vivecraft-порта (локальные переопределения) | единый профиль "vanilla" без привязки к протоколу |
| Paper + Vivecraft-плагин / Fabric с Vivecraft | `vivecraft:data` v11/v12 | есть до v11; нужен v12 (`ROOMSCALE_ATTACK`) | да (раздел 2.3) |
| Paper + VisorPlugin 0.2.0alpha | `visor:channel` v5: HANDSHAKE, SERVER_SETTINGS, POSE_DATA, состояния C2S, OTHER_* | нет | фазы 1-3 |
| Fabric/Forge/NeoForge с модом Visor | то же + игровые пакеты (SWING, TELEPORT, CRAWLING, CLIMBING, BLOCK_DAMAGE) | нет | фазы 1-3 |
| Сервер с обоими | выбирать одного, по умолчанию Visor или Vivecraft по настройке пользователя; одновременный ответ обоих не рассматривается (неподтверждено, как себя ведут плагины рядом) | - | политика в `net.rs` |
| Другие клиенты Vivecraft на Visor-сервере | видны как не-VR; не видят Horizon | - | нечего делать |
| Другие клиенты Visor на Vivecraft-сервере | не видят Horizon (и он их) | - | то же |
| Версия Visor-сети != 5 | сервер отключит игрока; Horizon должен показать понятную ошибку и не слать `visor:channel`, если сервер объявил другую версию (протокол объявления версии со стороны сервера у Visor отсутствует, сервер узнаёт версию из HANDSHAKE клиента) | - | поддерживать константу + таблицу версий, получаемую из spec |
| Horizon-сервер (Rust) | чтобы Visor-клиенты (другие игроки, Visor-мод на ПК) работали на нём: серверная часть протокола в `horizon-server` / LAN-хост | серверная часть Vivecraft (`vivecraft_server.rs`, `server.rs`) | фаза 5 |

## 6. Лицензии и credits (решено)

Решение пользователя: личный форк, не вливается в Horizon, будет опубликован; разрешение обеих сторон получено (см. 0.2). Что сделано: новый крейт `crates/horizon-visor` с `license = "MPL-2.0"` и `LICENSE-MPL-2.0`; в файлах с портированным кодом SPDX `MPL-2.0`, авторы ("Phoenix-Ra and Visor contributors") и коммит-источник (c01354f, плагин b4f3fc8); раздел "Visor" в `THIRD_PARTY_NOTICES.md`; строки `visor.credits.*` на экране About / Credits (VR Settings, `options_ui.rs::Page::VrCredits`; Vivecraft там уже был). MPL действует на уровне файла: файлы Horizon, которые только вызывают крейт, остаются MIT, файлы Vivecraft-порта LGPL. AtumVR (Apache-2.0) не брался. `CONTRIBUTING.md` не трогали.

## 7. Соответствие модулей Visor и целей в Horizon

Строки Java по пакетам `visor-core` (если не указано иное; `wc -l`). "Статус": reuse as-is / adapt / rewrite / new / drop.

| Модуль Visor (Java LOC) | Цель в Horizon | Статус |
|---|---|---|
| `api/common/network` + `core/client/network` + `core/server/network` (около 4 300) | `crates/horizon-visor/src/network/*` (канал, `VisorCorePayloadID`, 28 пакетов, буферы поз), общий трейт с `horizon-vivecraft` | **new** (reuse: `wire.rs` буферы, схема версий/хэндшейка по образцу `network/mod.rs`, около 500 строк адаптировать) |
| `api/server` + `VRServerSettings` + `ServerConfig` (около 600) | `horizon-visor/src/server_settings.rs`: разбор текстового `SERVER_SETTINGS`, значения по умолчанию "vanilla/dedicated" | **new**; `settings.rs` Vivecraft-порта как образец |
| `core/server/player` + `mixin/common` (серверная логика: поза, лазанье, ползание, телепорт, удары, щит, PvP, взрывы криперов) (около 3 500 серверной части) | `horizon-server` / LAN: `game/visor_server.rs` | **adapt** (идеи `vivecraft_server.rs`/`server.rs`: аим, щит, лук), в основном **rewrite** под Visor-настройки (`two_handed`, `better_swinging`) |
| `core/client/player` + `player/pose` + `body` + `height` + `api/client/player` (около 7 500) | `horizon-visor/src/player/*` (поза TICK/RENDER, VRBodyType, рост, remote игроки) | **adapt**: `vr_data.rs`, `player.rs`, `math.rs`, `player_model.rs` перенимаем; FBT/суставы **new** |
| `core/client/render/player` (VRPlayerRendererFull/HandsOnly, 1 600) | `mobs/anim.rs` + модель в `player_model.rs` | **adapt/rewrite** (меш и скиннинг в Horizon свои) |
| `core/client/tasks` (3 242) и `input/actions` (1 461) | `horizon-visor/src/tasks/*`, `headset_trackers.rs`, `horizon-client-input/headset.rs` | **adapt**: `trackers/{swing,teleport,climb,body,moves}` перенимаются по сути; `TaskRoomConsume`, `TaskHotBar`, покачивание Visor **rewrite** |
| `core/client/input` (наборы действий, редирект мыши, тредмилы, 2 674) | `horizon-xr/src/actions.rs`, `horizon-client-input/headset.rs` | **reuse** (платформа), тредмилы **drop** (на Quest нет) |
| `core/client/provider/openxr` + AtumVR (1 515 + внешний код) | `horizon-xr` | **reuse as-is** (Horizon уже на OpenXR под Quest); добавить hand tracking (XR_EXT_hand_tracking) и body tracking (XR_FB_body_tracking): **new** |
| `core/client/settings` + `api/client/settings` + пресеты (около 3 000) | `horizon-client-render/src/vr_options.rs`, `options_ui.rs`, таблицы в `horizon-vivecraft` | **adapt**: общий формат конфига `vr-settings.json`; пресеты **new** (малый объём) |
| `core/client/gui` (17 000) + `api/client/gui` (15 000): оверлеи, экраны, виджеты | UI Horizon (`options_ui.rs`, `horizon-xr/panel.rs`) | **drop** виджетную систему и экраны (замена: UI Horizon); **new**: настраиваемые оверлеи HUD и клавиатура (около 3 000 Rust) |
| `core/client/gui/overlays/builtin/keyboard` (7 файлов) | `game/vr_keyboard.rs`, `horizon-vivecraft/keyboard.rs` | **reuse** клавиатуру Vivecraft-порта, раскладки Visor только по желанию |
| `core/client/render` (VRRendererBase, таргеты, камеры, 12 200) | `horizon-client-render`, `horizon-xr` | **drop** (замена: нативный wgpu-рендер); **new** только фаза скрытой области/масштаба рендера, если ещё нет |
| `core/client/render/decoration` (6 538) + `shaders` (426) + `vr_*.fsh` | рендер рук/предметов `headset_hands.rs`, эффекты (виньетка в блоке, тыква, портал, телепорт-точка) | **adapt** рук, **new** эффектов в wgsl (около 1 500) |
| `mixin/*` (8 883), `mcversion/1.20.1` (7 278) | - | **drop** (нет Minecraft Java; поведение учтено в соответствующих модулях) |
| `compatibility/*` (около 6 400 + `api/compatibility` 6 600) | - | **drop** (Sodium, Iris, Xaero, Create, Voxy, Flashback, DH ... не существуют в Horizon); исключение: запись/воспроизведение (`RecorderModHelper`) **drop** |
| `api/**` публичный API + события + аддоны (около 4 000 существенного) | `horizon-mods` / `horizon-mod-api-gen`: события VR для нативных модов | **new**, маленький срез (около 1 500) |
| VisorPlugin (9 778 Java) | не Horizon; используется как сервер для тестов | **drop** (брать готовый jar/сборку, исходники не портировать) |

## 8. Оценка объёма (Rust)

### 8.1. Метод

1. Берём Java-строки только тех подсистем, которые имеют смысл в Horizon (см. таблицу 7): примерно **42 000** из 106 023. Остальное: миксины 8.9K, совместимость 13K, дубликаты `mcversion` 7.3K, виджетный GUI около 25K, рендер-пайплайн Minecraft около 12K, загрузчики 2.3K (пересекаются, итоговая сумма неточная).
2. Коэффициент Rust/Java: **0.45 - 0.78**. Обоснование: lombok (`@Getter`), records, javadoc и комментарии "stonecutter" (`//? if`) занимают заметную долю Java; Rust добавляет явные типы и `match`, но убирает интерфейсы и `*Impl`-пары. Проверка на собственных данных: калибровки по текущему порту у меня нет (соответствие файлов Vivecraft и Rust-файлов не считал), поэтому коэффициент это допущение, не измерение.
3. Валовые строки по подсистеме: `Java * коэффициент`.
4. Вычитаем существующий переиспользуемый код Horizon (низкая оценка: только платформа и каркас; высокая: ещё перенимаемая логика).
5. Диапазон: низ = валовый низ минус высокая переиспользуемость; верх = валовый верх минус низкая.

### 8.2. Таблица

| Подсистема | Java в охвате | Валовые Rust (низ - верх) | Переиспользуемо | Чистые новые/переписанные |
|---|---|---|---|---|
| Сеть и протокол Visor | 4.3K | 1.6K - 2.6K | 0.5K | 1.1K - 2.1K |
| Серверная логика (Horizon-сервер/LAN) | 4.0K | 2.0K - 3.5K | 1.0K - 1.8K | 0.2K - 2.5K |
| Поза, удалённые игроки, FBT/руки | 7.0K | 3.0K - 5.0K | 0.8K - 1.5K | 1.5K - 4.2K |
| Задачи и ввод (локальный геймплей) | 6.0K | 2.5K - 4.0K | 1.5K - 2.5K | 0.0K - 2.5K |
| Настройки (клиент, пресеты) | 3.0K | 1.5K - 2.5K | 0.5K - 0.8K | 0.7K - 2.0K |
| Платформа (hand/body tracking в `horizon-xr`) | 1.5K (+AtumVR) | 0.6K - 1.2K | 0 | 0.6K - 1.2K |
| Оверлеи HUD, клавиатура, меню (срез GUI) | 6.0K | 2.0K - 4.0K | 0.7K - 1.0K | 1.0K - 3.3K |
| Рендер рук/эффекты/шейдеры | 8.5K | 3.0K - 5.5K | 0.6K - 1.0K | 2.0K - 4.9K |
| События и API для модов | 2.0K | 1.0K - 2.0K | 0 | 1.0K - 2.0K |
| Тесты, перевод, клей | - | 1.5K - 2.5K | 0 | 1.5K - 2.5K |
| **Итого валом** | ~42K | **18.7K - 32.8K** | 5.6K - 9.1K | |

**Итог: около 10 000 - 27 000 новых строк Rust, центральная оценка около 18 000** (низ = 18.7 - 9.1, верх = 32.8 - 5.6, центр = 25.8 - 7.3). Разброс большой, потому что: коэффициент допущение; степень паритета GUI/оверлеев Visor неопределена (полная копия виджетной системы нереальна и не нужна); серверная часть зависит от решения, делать ли Horizon-сервер совместимым с Visor-клиентами. Плюс протокол Visor нестабилен: ожидаемая переделка 10-20% после выхода 0.7.0.

Отдельно (не входит): добавление Vivecraft 1.3.18 (`ROOMSCALE_ATTACK`) около 250 строк (раздел 2.3).

## 9. Фазы (в порядке приоритета)

### Фаза 0. Снята
Решение о лицензии принято пользователем (см. 0.2 и 6). Отдельной фазы и вопросов разработчикам Visor нет.

### Фаза 1. Каркас двух протоколов + обновление Vivecraft до 1.3.18 (2.0K - 3.0K)
- Цель: трейт `VrProtocol { Vivecraft(v12), Visor(v5), None }`, выбор по каналам сервера, единый локальный профиль "vanilla". Перенос `horizon-vivecraft/network/*` без изменений поведения, добавление `RoomscaleAttack`.
- Объём: `horizon-visor/network` (канал, enum идентификаторов, HANDSHAKE, SERVER_SETTINGS, POSE_DATA C2S и состояния C2S), `net.rs` выбор протокола.
- Риски: определить на клиенте, какой протокол поддерживает сервер до отправки HANDSHAKE (неизвестно, объявляет ли Visor сервер канал в конфигурационной фазе - проверить); кик при несовпадении версии.
- Проверка на гарнитуре: подключиться к Paper+VisorPlugin 0.2.0alpha (`b4f3fc8`) и к Paper+Vivecraft: в логе "handshake accepted", сервер видит VR-игрока (`/visor status`), нет кика; ванильный сервер без изменений.

### Фаза 2. Поведение по настройкам сервера и игровые пакеты (2.5K - 4.0K)
- Цель: клиент исполняет `two_handed`, `better_swinging`, `roomscale_shield`, `room_crawling/climbing`, `supported_movement`, лимиты телепорта, `supported_height_mode`.
- Объём: `SWING_ATTACK/SWING_BLOCK/TELEPORT/CRAWLING/CLIMBING`, `OFFHAND_SLOT`, `ROTATION_Y`, `BLOCK_DAMAGE`, адаптация трекеров из `horizon-vivecraft`.
- Риски: различие семантики (например, `better_swinging` и скорость добычи `swingingMiningSpeed`); Quest-контроллеры.
- Проверка: на VisorPlugin-сервере бить мобов и блоки рукой, поднять щит, телепорт с лимитами, ползти и лазать; сравнить с Visor-клиентом на ПК.

### Фаза 3. Другие VR-игроки (3.0K - 5.0K)
- Цель: видеть игроков Visor и показывать себя им: `OTHER_VR_*`, `VRBodyType`, рост, масштаб мира, угол ружья.
- Объём: приём `START_TRACKING` с отложенной обработкой, поза рук/головы, рендер модели (`player_model.rs` адаптировать); отправка состояния только при изменении.
- Риски: формат моделей разных `VRBodyType`; нагрузка на кадр CPU.
- Проверка: второй игрок с Visor на ПК на том же сервере: головы/руки двигаются, масштаб, левша; замер времени кадра с 1, 4 и 8 игроками.

### Фаза 4. Отслеживание рук и тела на Quest (1.5K - 2.5K)
- Цель: XR_EXT_hand_tracking и XR_FB_body_tracking, отправка суставов/трекеров, если сервер разрешил (`bodyTrackersSupported`, `handTrackersSupported`).
- Риски: расширения доступны не на всех прошивках; трафик (в настройках Visor предупреждение).
- Проверка: включить трекинг рук в Quest, на втором клиенте видны пальцы; в `/visor hands` растёт счётчик.

### Фаза 5. Серверная часть в Horizon (2.0K - 3.5K, по решению)
- Цель: Horizon-сервер и LAN-хост понимают `visor:channel`, чтобы Visor-клиенты на ПК играли с Horizon.
- Риски: не обязательна для "клиент на серверах как настоящий мод"; нестабильность протокола.
- Проверка: Visor-мод на ПК подключается к Horizon-серверу, виден как VR.

### Фаза 6. Настройки, оверлеи, клавиатура, пресеты (3.0K - 6.0K)
- Цель: паритет по пользовательским настройкам (профили, HUD-оверлеи, раскладки), насколько нужно на Quest.
- Риски: размер; ограничивать срез по реальному использованию.
- Проверка: пользовательская - открыть, поменять, перезапустить, значения сохраняются; без сырых ключей в тексте.

### Фаза 7. Рендер рук и эффектов (3.0K - 5.5K)
- Цель: виньетка в блоке, наложения, телепорт-точка, рука с эффектами, главное меню.
- Проверка: визуальная, 90 Гц удерживается; время кадра до/после.

### Фаза 8. События/API и доработка (1.0K - 2.0K + тесты)
- Цель: события VR для нативных модов Horizon (`horizon-mods`), документация.
- Проверка: пример мода, получающий события.

### Статусы фаз (реализация на ветке `visor-port`, без устройства)
| Фаза | Статус |
|---|---|
| 0 | снята решением |
| 1 | ported (тесты на литералах) / выбор протокола ported-untested |
| 2 | ported-untested (логика задач с тестами) |
| 3 | ported-untested (модель рук; пальцы и ноги других игроков не рисуются) |
| 4 | ported-untested (`horizon-xr/src/tracking.rs`; разрешение на тело запрашивать у системы вручную) |
| 5 | ported-untested (круговой тест состояния в памяти) |
| 6 | срез: страница Visor, два пресета; виджеты и оверлеи Visor skipped |
| 7 | срез: виньетка у блока; эффекты рук skipped |
| 8 | skipped (хуки native-модов это версионируемый ABI) |
| Производительность | см. `docs/visor-port.md`, "Performance switches" |

### Параллельный трек производительности (не зависит от Visor)
- Single-pass multiview; общая подготовка кадра; Application SpaceWarp и foveation; пропуск лишних обновлений (раздел 3.5). Замеры: время CPU кадра в `horizon-bench`/логах на Quest, критерий - стабильные 120 Гц без падений при 4+ игроках. Объём вне оценки выше.

## 10. Vivecraft теперь в прошлом, Visor текущая цель

- Код Vivecraft-порта **не удалять и не ломать**: он компилируется и работает, но не развивается. Обновление до 1.3.18 / `ROOMSCALE_ATTACK` (раздел 2.3) **не делается**.
- Выбор протокола: настройка VR Settings > Visor Settings > "VR Protocol" (`horizonVrProtocol`): Auto / Visor / Vivecraft. **Auto**: после логина клиент шлёт Visor `HANDSHAKE`, а Vivecraft `VERSION` придерживает; пришёл ответ - Visor (на сервере с обоими каналами выбирается Visor); 40 тиков без ответа - шлётся `VERSION`. `minecraft:register` в конфигурационной фазе называет оба канала.
- Локальные механики и настройки ориентируются на Visor (`visor_mechanics()`: настройка не Vivecraft и сервер не ответил как Vivecraft), там, где он отличается: взмах (`TaskSwing`), ползание, лазание, телепорт, пределы, порог скорости, режим роста, тело. На ванильном сервере единый профиль `ServerSettings::vanilla_server()` (`joinedDedicatedServer`).
- Общие типы: поза идёт из одного `vr_local_pose()` в оба протокола; профиль возможностей сервера для механик читается через `vr_server_settings()` (Visor) или поля `vivecraft.net` (Vivecraft); тонкого трейта не потребовалось.

## 11. Открытые вопросы (закрыты или не блокируют)

Мейнтейнеру Horizon: решено пользователем (лицензия, серверная часть нужна, Visor по умолчанию в Auto, Vivecraft 1.3.18 не делаем). Разработчикам Visor: вопросы 1-6 больше не блокируют работу.
Ответ на главный технический вопрос из прошлой версии ("как клиент узнаёт, что сервер поддерживает Visor"): **никак заранее**. Настоящий клиент шлёт `HANDSHAKE` каждому серверу в конце `handleLogin` и считает сервер Visor-сервером, когда пришёл пустой ответ `HANDSHAKE`. Регистрация канала (`minecraft:register`) нужна только плагинам Paper/Spigot, чтобы они слали по каналу; Horizon регистрирует оба канала в конфигурационной фазе. Что делает сервер с клиентом, чья версия сети не 5: мод кикает `visor.messages.network_mismatch` (переводимый ключ, с двумя числами), плагин - строкой "Visor network version mismatch: client X, server Y". Horizon всегда шлёт 5 и держит оба ключа в своих lang-файлах.

## 12. Что не проверено (на момент реализации)

- Всё, что относится к Quest и к настоящим серверам Visor/VisorPlugin: устройства и серверов не было (см. чеклист в `docs/visor-port.md`).
- Точный текст `Config.toPlaintext()` мода (библиотека AtumConfig не читалась): разбор принимает любой плоский YAML.
- Поведение серверов на неизвестный канал; одновременная установка двух плагинов.
- Нумерация суставов `XR_FB_body_tracking` / `XR_META_body_tracking_full_body` взята из `openxr-sys` (константы совпадают с реестром).
- Скорость Visor относительно Vivecraft (в коде приёмов не видно, раздел 3.4).

## Источники

- https://github.com/VisorModStudio/Visor (коммит c01354fad8ac03a016c249239a04b07adeb36476)
- https://github.com/VisorModStudio/VisorPlugin (коммит b4f3fc8be0e67f517c3e98c07ac1b5510eaf3d24)
- https://github.com/VisorModStudio/Visor/wiki
- https://github.com/VisorModStudio/Visor/releases/tag/0.6.1
- https://modrinth.com/mod/visor
- https://github.com/Phoenix-Ra/AtumVR
- https://github.com/Vivecraft/VivecraftMod (локально: `<workspace>/VivecraftMod` 1.3.15, `<workspace>/VivecraftMod-26.3-1.3.18`)
- https://www.mozilla.org/MPL/2.0/
