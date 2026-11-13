# Quest / Adreno: аудит и экспериментальные оптимизации, 13 ноября 2026

Исходная база аудита: `e5731a7cda` (`ytpk/horizon-xr`). Перед отправкой также
подхвачены актуальные изменения main `d86506b380`: exporter bundle / release packaging. Изменения подготовлены Codex по запросу
владельца задачи, включая разрешённые потери качества. Проверка — только Linux,
синтетические данные и программный Vulkan. Новых замеров Quest здесь **нет**.

## Цель и бюджет

24 чанка настоящего рельефа остаются 24 чанками. Подмена на RD 8 + LOD 24 в этих
профилях не производится. LOD-дистанцию для сравнения следует выставить в 0 отдельно.

| Цель | Интервал кадра | Что должно помещаться |
| --- | --- | --- |
| 72 FPS | 13,89 мс | критическая CPU-работа и полный стереокадр GPU, с запасом для рантайма |
| 90 FPS | 11,11 мс | то же |
| 120 FPS | 8,33 мс | то же; частота должна поддерживаться и реально включиться у рантайма |

По **прежним**, а не новым измерениям [GPU handoff](quest-gpu-handoff.md), лёгкий
ракурс RD 24 занимал около 10,4 мс GPU, тяжёлая трава — около 21 мс без динамического
снижения разрешения. Для второго случая нужен примерно 34% выигрыш только до
13,89 мс и около 60% до 8,33 мс, ещё до запаса на композицию. Небольшая оптимизация
арифметики шейдера сама по себе этого не обеспечит. GPU-время и FPS не взаимозаменяемы:
есть CPU-работа, ожидания и pacing. Ожидание OpenXR не равно загрузке CPU.

## Результаты просмотра коммитов ytpk

Рассмотрена история порта: 24 коммита ytpk от `4fd9a93dd6` до `e5731a7cda`, с
углублённой проверкой текущих путей terrain/mesher/streaming/worker pool и условий
включения экспериментальных режимов. Это не утверждение о проверке всей игровой
логики или каждого драйвера Quest.

| Коммиты / область | Вывод |
| --- | --- |
| `946c885498`: split opaque/cutout, emissive table, multiview, foveation | Сохраняем. Наличие discard в общем solid pipeline дорого для Adreno. Замены уже есть в порте; повторное внедрение не требуется. |
| `195e793fad`: split streams | Сохраняем как отдельный эксперимент. 16+8 байт вместо 24 interleaved — тот же суммарный объём, но меньший position stream при binning. Это не сжатие всего меша. Native displacement с этим layout ограничен. |
| `0ad1124c7b`: lean varyings, merge, plants | Lean opaque/cutout уже включены на подходящем пути. Строгое merge мало помогает при разных AO, свете и поворотах UV; добавлен отдельный более грубый режим. Shrink не гарантирует сохранения краёв на мелких mip-уровнях. |
| `157b4d711f`: Experiments UI | Использован существующий механизм. Добавлены три переведённых переключателя; применение после перезапуска. |
| `81af30d775`, `e5731a7cda`: cave visibility, Adreno shader fixes | Сохранены. Переполнение очереди должно инвалидировать старую видимость; иначе открытая стена может оставаться скрытой. Существующие регрессионные тесты остаются. |
| `08cbe355a1`, `cb9760ef25`, `a92f513ac6`: water preparation | Кэширование и отсечение воды уже есть. В прежнем тяжёлом травяном ракурсе translucency почти не влияла; дальнейшая работа только над водой не решает этот случай. |
| `2cd0287122`, `5b1636624f`: native foveated arrays | Сохраняем opt-in и fallback. Обход density-map для правого глаза и проблемы снимков экрана требуют устройства; desktop Vulkan их не подтверждает. |
| `c20da6da42`, `473bbf2192`: scale, performance controls | Масштаб глаза линейный относительно рекомендации OpenXR. Нельзя слепо ставить `HORIZON_XR_SCALE=1.0`: это может резко увеличить число пикселей относительно текущих 0,4–0,5. Профили ниже сохраняют выбранный размер. |
| `0195321bcb`, `d86506b380`: свежие изменения main | Просмотрены и включены: release пересобирает exporter bundle, APK packaging отвергает архив без нужных exporter class/source. Rust-рендер не меняется. |
| affinity / shared jobs | При включённом закреплении render thread дочерние workers наследуют P-only mask. Без явного восстановления маски добавление потоков не возвращает E-ядра. Добавлен экспериментальный hybrid mode. |

Ключевые оставшиеся ограничения: отсечение граней работает по направлениям, но
скрытую за рельефом геометрию оно не устраняет; GPU culling сейчас несовместим с
split-solid и stereo; fragment-storage recall на Quest уже отмечен сломанным.
Отсутствие валидационной ошибки не доказывает работоспособность indirect draw.

## Что добавлено

### Более грубое объединение граней

`HORIZON_QUEST_AGGRESSIVE_MESH=1`, или **Aggressive Terrain Merging** в Experiments.
Включает и само объединение граней.

Перед существующим greedy merge целые не перекрытые opaque-грани получают:

- плоское среднее RGB с шагом 32 и свет с шагом четыре уровня;
- единое пространственное направление UV вместо случайных поворотов;
- одинаковый старт углов при сохранении winding.

Позиции, sprite layer, направление грани и биты emission сохраняются. Cutout,
translucent, неполные UV и грани под распознанным coplanar overlay не упрощаются.
Меши с per-quad sources шейдерных паков не объединяются. Используются прежние
кодирование повторяющихся UV, worker scratch и сортировка сторон.

Потери: AO, оттенки и свет становятся грубее, вариация текстуры уменьшается.
Интерполяция fog/dynamic lights на большой грани также может отличаться; возможны
T-junctions. Качество намеренно ниже. Синтетический плоский участок может перейти
из 256 shaded/rotated quads в один; это **не** оценка сокращения реального мира.
Само упрощение добавляет CPU-работу в meshing worker. Выигрыш зависит от того,
сколько граней реально объединится, и нуждается в A/B.

### Плотность растений до построения геометрии

`HORIZON_QUEST_PLANT_DENSITY=0..100`, или **Plant Density (%)**. По умолчанию 100.
50 оставляет примерно половину моделей; 0 убирает подходящие модели целиком,
включая ближнюю траву — основной дорогой случай из прежних измерений.

Выбор использует стабильный hash мировых X/Z. Все faces и обе половины высокого
растения остаются вместе, без перетасовки при движении камеры или remesh. Плотность
читается один раз на секцию. Удаление выполняется до освещения faces и выпуска
вершин: сокращаются CPU meshing, upload, vertex и cutout fragment work.

Защищены светящиеся блоки, block entities, жидкости, смешанные модели и source meshes
шейдерных паков. Блоки, коллизии, сервер и сохранения не меняются. Классификация
геометрическая: целиком неосевой cutout-декор пользовательского пака тоже может
исчезнуть. Это не список только травы по именам.

Существующий **Plant Distance** можно применять независимо. Его section-based
появление растений остаётся заметным; неправильные бесконечные значения теперь
отбрасываются.

### Выборка освещения без фильтрации

`HORIZON_QUEST_NEAREST_LIGHTMAP=1`, или **Fast Lightmap Sampling**.
В terrain shader sampler заменяется на один `textureLoad` ближайшего значения
16×16 lightmap. Координаты ограничены 0..15. Динамический свет остаётся, но меняется
ступенчато. Возможны полосы освещения и разница с освещением entities.

Этот режим **не гарантирует** ускорение: аппаратная фильтрация может оказаться
дешевле дополнительной арифметики. Он выключен по умолчанию и сравнивается отдельно.
Проверяется не только парсинг: GPU-тест читает известные пиксели filtered/nearest
в mono и обоих слоях multiview.

### Использование P- и E-ядер

`HORIZON_HYBRID_WORKERS=1` вместе с существующим `HORIZON_PIN_BIG_CORES=1`.

1. До закрепления foreground сохраняется исходная разрешённая affinity mask.
2. Топология определяется по `cpu_capacity`, иначе по max frequency, на одной
   шкале для всех CPU. Прежнее смешивание kHz и capacity при частичных данных устранено.
3. Два самых быстрых доступных P-ядра оставляются render/server/submit. Общий пул
   получает остальные P- и E-ядра, по worker на ядро. Его автоматический размер
   учитывает эту схему, а явные лимиты потоков сохраняются.
4. Уже асинхронные cave-visibility search и sprite compression получают маску E-ядер.
   Meshing, lighting и generation используют общий пул. Нового ожидания этих
   результатов в render thread не вводится; немедленная подготовка draw lists и
   OpenXR submit не переносятся на E.

Нет фиксированных номеров CPU и отдельного бесконтрольного пула на каждую подсистему.
При неизвестной/неполной топологии, менее трёх P-ядер или однородном CPU сохраняется
обычное планирование. Отказ affinity не останавливает приложение и попадает в лог.
Маска foreground и маски workers выводятся отдельно: план не заменяет проверку
фактического выполнения на шлеме.

Это разделение ресурсов, а не обещание ускорения: E-ядра могут замедлить отдельный
job, общий pool допускает на них и urgent jobs, а больше активных ядер может усилить
нагрев и конкуренцию за память. Сравнивать следует загрузку/полёт и p95/p99, а не
только неподвижный ракурс. При явном `HORIZON_JOB_THREADS` возможна осознанная
переподписка ядер; для первого сравнения лучше Auto.

## Внешние реализации: что проверено и что применимо

Проверены лицензии и заголовки выбранных файлов. Ниже постоянные ссылки на
рассмотренные ревизии. Чужой исходный код в этом изменении не копировался: новые
алгоритмы оригинальные, внешние реализации использовались как проверка подходов
и ограничений. Filament — прямой Vulkan, не приложение на wgpu.

| Проект | Изученные места | Применение / ограничение |
| --- | --- | --- |
| Bevy, MIT, `41a1282e376c` | [GPU preprocessing](https://github.com/bevyengine/bevy/blob/41a1282e376ce0dcd9808e1b3f6c071803363eb4/crates/bevy_render/src/batching/gpu_preprocessing.rs), [pipeline cache](https://github.com/bevyengine/bevy/blob/41a1282e376ce0dcd9808e1b3f6c071803363eb4/crates/bevy_render/src/render_resource/pipeline_cache.rs), renderer и GPU preprocess | Многоуровневые gates по limits, features и драйверу; background compilation. Bevy исключает Adreno ≤730, кроме 720, из своего GPU preprocessing. Это не означает запрета всего compute на Quest, но перенос desktop Hi-Z целиком необоснован. В Horizon cache и submit thread уже есть; добавлять конкурирующие механизмы ради самого переноса не нужно. |
| Filament, Apache-2.0, `65e5d873f188` | [VulkanDriver](https://github.com/google/filament/blob/65e5d873f1884748ef205ed72335426393757347/filament/backend/src/vulkan/VulkanDriver.cpp), [JobSystem](https://github.com/google/filament/blob/65e5d873f1884748ef205ed72335426393757347/libs/utils/src/JobSystem.cpp) | Явные affinity/priority и discard/transient attachments. В новом режиме реализовано явное распределение собственных workers. Для depth-array multiview нельзя просто добавить TRANSIENT: wgpu-core 30.0.1 запрещает transient texture с более чем одним слоем. |
| wgpu, MIT/Apache-2.0, `c92258fcae03` и vendored 30.0.1 | [Quest indirect issue #8801](https://github.com/gfx-rs/wgpu/issues/8801), Vulkan allocator и texture validation в текущей зависимости | На Quest рекламируемые indirect features не доказывают исправность пути. В чужом отчёте выключение indirect validation не помогало, в прежних замерах этого форка помогало. Поэтому глобальное выключение защиты и включение GPU culling не сделаны. |
| Khronos Vulkan Samples, документация | [Render attachments](https://docs.vulkan.org/samples/latest/samples/performance/render_passes/README.html), [subpasses](https://docs.vulkan.org/samples/latest/samples/performance/subpasses/README.html) | Сокращать store/load и сохранять данные внутри tile полезно. В порте уже объединён screen pass; маскированный direct native stereo остаётся предпочтительным при работе на устройстве. Subpass API напрямую не переносится через текущий wgpu. |

Следующий большой GPU-шаг: совместимый со split-solid stereo Hi-Z/compute culling,
с двумя глазами, консервативным результатом и проверкой compute→indirect на
реальных Quest 2 и 3/3S. Это отдельное изменение; программный Vulkan не может
подтвердить исправление известного драйверного сбоя.

## Профили и воспроизводимое сравнение

Файлы для добавления к существующему `launch.txt` после резервной копии:

- [rd24-balanced.launch.txt](../packaging/quest/profiles/rd24-balanced.launch.txt):
  цель 72 Гц, 50% растений, aggressive merge, medium foveation, hybrid workers.
- [rd24-throughput.launch.txt](../packaging/quest/profiles/rd24-throughput.launch.txt):
  запрос 120 Гц, подходящие растения скрыты, nearest lightmap, high foveation и split streams.

Это наборы для испытаний, **не измеренные гарантии FPS**. В обоих нужно включить
`horizonMultiview` и `horizonFoveatedMultiview` в сохранённых VR-настройках и
перезапустить приложение. Проверить лог: фактически использованный stereo path,
размер глаз, текущую частоту, отсутствие affinity errors. Shader packs/HDR/OIT и
другие несовместимые эффекты могут вернуть separate-eye rendering. Профили их
настройки не переписывают. Не удалять существующие world/account параметры.

Для измерения отдельных изменений начинать с базового коммита и той же позиции,
времени, погоды, RD 24, LOD 0 и **одинакового размера глаз**. Dynamic resolution
выключить в обеих сборках. По одному сравнить density 100/50/25/0, aggressive merge,
nearest lightmap, split streams и hybrid workers. Сначала одинаковая частота,
затем отдельно 72/90/120. В финальном quality tradeoff можно снизить размеры глаз
в VR UI ещё на 10–15% относительно текущих, записав точные пиксели.

Проверять обе картинки: горизонт, пещеры, T-junctions при движении, близкий свет,
декор, вода и переходы чанков. Для CPU — загрузка мира, полёт, очередь meshing,
p95/p99, память и длительный прогрев. На foveated array обычный screencap ранее был
чёрным; диагностический снимок делать отдельным прогоном с выключенной фовеацией.
Без перезапуска смена meshing switches не является корректным A/B.

При подтверждённом CPU-limit интегрированного сервера отдельно можно сравнить
`simulationDistance:5` или `6` в options с `renderDistance:24`. В коде это независимые
параметры: снижается область тиков мира, видимая дальность остаётся прежней.
Это меняет работу дальних механизмов/мобов, поэтому готовые профили simulation
принудительно не меняют. Предварительная генерация испытательного мира также
позволяет отделить meshing/render от пиков world generation.

## Локальная проверка

Обычный полный прогон renderer выполнен с Rust 1.95.0 и отключённой debug information;
финальные целевые проверки и Clippy — с Rust 1.99.1. После последней оптимизации
CPU-прохода greedy merge повторён весь набор mesher, а не только новый тест.

| Проверка | Результат |
| --- | --- |
| Renderer library, полный прогон | 2187 passed, 4 failed, 76 ignored; подробности ниже |
| Mesher, финальная версия | 76 passed, 25 ignored |
| Новые quality-тесты, включая настоящий GPU readback | 3 passed; filtered/nearest в mono и двух глазах |
| Дополнительно включённые stereo GPU-тесты | 4 passed: external array, independent projections/copies, conservative stereo boxes, per-eye masks |
| Split/interleaved streams, GPU pixels | 1 passed: одинаковые пиксели обоих глаз |
| Threads / Vivecraft, финальная версия | 31 + 141 passed; Linux affinity inheritance/recovery и синтетическая P/E-топология включены |
| Renderer doctests | 8 passed |
| Python suite | 831 tests, OK, 33 skipped без подготовленных игровых данных |
| Clippy изменённых крейтов, all targets, `-D warnings` | Passed; отдельное старое dependency warning у vendored wgpu-core остаётся |
| Client Clippy, `--no-default-features --all-targets -D warnings` | Passed |
| Public tree, layers, file size, rustfmt, whitespace | Passed |

Синтетический участок: exact merge оставляет 256 quads, aggressive — один. В
неоптимизированном Linux microbenchmark 1000 итераций заняли около 0,72 с и 1,31 с
соответственно. Это заведомо стрессовый участок и **не** runtime frame timing:
CPU-стоимость aggressive выше, результат не переносится на Quest как коэффициент FPS.

Четыре падения полного renderer-прогона:

- `crash::tests::a_panic_becomes_a_report` и
  `crash::tests::the_debug_crash_is_reported_as_vanillas`: нет требуемых file/line
  в backtrace при выбранном `DEBUG=0`;
- `gfx::hdr::tests::sdr_screenshots_roll_off_the_world_and_keep_the_hud_as_drawn`:
  llvmpipe дал красный `0.9038086` вместо `0.89990234`;
- `gfx::room_scene::tests::the_room_is_drawn_from_every_side_before_and_after_the_real_textures`:
  строгая проверка пикселя получила `[198,29,17]` вместо `[194,28,17]`.

Эти имена/ограничения уже описаны в [предыдущем аудите](quest-adreno-audit-2026-11-12.md).
Соответствующие реализации crash/HDR/room здесь не менялись; отдельной полной
пересборки базового коммита в этой сессии не было. Тесты не отключались и их
допуски не расширялись. Поэтому полный набор не обозначен зелёным.

`pr_ready.py --base origin/main --quick` также запущен. Общая проверка остаётся
красной: проверка addresses находит существующие endpoints в неизменённых
`options_ui.rs`, `game/quest_room.rs`, `horizon-quest-launcher/src/news.rs`, а
workspace-default сборка упирается в отсутствующий Linux `alsa.pc`; отдельные
launcher checks блокирует отсутствующий `fontconfig.pc`. Это не
заменено утверждением о пройденной сборке всего workspace с default features.

Для воспроизведения:

```sh
cargo +1.99.1 clippy -p horizon-client-render -p horizon-threads -p horizon-vivecraft --all-targets --locked -- -D warnings
cargo +1.99.1 test -p horizon-threads -p horizon-vivecraft --locked
cargo +1.99.1 test -p horizon-client-render --lib --locked --no-default-features
cargo +1.99.1 test -p horizon-client-render --lib --locked --no-default-features gfx::quest_quality:: -- --include-ignored
cargo +1.99.1 test -p horizon-client-render --lib --locked --no-default-features quality_mesh_benchmark -- --ignored --nocapture
python3 -m unittest discover -s tests
```

Для GPU-проверок требуется доступный Vulkan ICD; в этой среде использовался
Mesa lavapipe 25.0.7, LLVM 19.1.7. Подготовленных ресурсов Minecraft и шлема нет.
Android APK не собран: `build-apk.sh --check` останавливается на отсутствующем
`javac`; проверки Rust/Linux не эквивалентны проверке Android ABI и рантайма.
