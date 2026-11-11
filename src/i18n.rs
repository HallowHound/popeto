//! Interface language. Strings are written in English in the code and looked up here;
//! anything without a translation stays English.

use std::cell::Cell;

/// (code, name shown in the language list)
pub const LANGUAGES: [(&str, &str); 2] = [("en", "English"), ("ru", "Русский")];

thread_local! { static LANG: Cell<usize> = const { Cell::new(0) }; }

/// Windows UI language mapped to one of ours (English when unsupported).
pub fn system_code() -> &'static str {
    let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() } & 0x3FF;
    match id {
        0x19 | 0x22 | 0x23 => "ru", // Russian, Ukrainian, Belarusian users most likely read Russian
        _ => "en",
    }
}

/// `setting` is "auto" or a language code. Returns the code actually used.
pub fn set(setting: &str) -> &'static str {
    let code = if setting.eq_ignore_ascii_case("auto") || setting.is_empty() { system_code() } else { setting };
    let i = LANGUAGES.iter().position(|l| l.0.eq_ignore_ascii_case(code)).unwrap_or(0);
    LANG.with(|l| l.set(i));
    LANGUAGES[i].0
}

pub fn code() -> &'static str { LANGUAGES[LANG.with(|l| l.get())].0 }

/// Fill `{}` placeholders in order.
pub fn tf(s: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut parts = t(s).split("{}");
    if let Some(first) = parts.next() { out += first; }
    for (i, p) in parts.enumerate() {
        if let Some(a) = args.get(i) { out += &a.to_string(); }
        out += p;
    }
    out
}

pub fn t(s: &'static str) -> &'static str {
    if code() != "ru" { return s; }
    match s {
        // tray menu
        "Popeto - {} pet(s)" => "Popeto - питомцев: {}",
        "Settings..." => "Настройки...",
        "Choose pets..." => "Выбрать питомцев...",
        "Add pets from Shimeji zip..." => "Добавить питомцев из архива Shimeji...",
        "Choose a Shimeji zip, or any file inside an unpacked Shimeji folder" => "Выберите архив Shimeji или любой файл внутри распакованной папки Shimeji",
        "Shimeji pets (zip, Shimeji-ee.jar, png)" => "Питомцы Shimeji (zip, Shimeji-ee.jar, png)",
        "All files" => "Все файлы",
        "Get more pets" => "Найти ещё питомцев",
        "Pets" => "Питомцы",
        "Life" => "Жизнь",
        "Toys" => "Игрушки",
        "Drop a cookie" => "Бросить печенье",
        "Release a balloon" => "Запустить шарик",
        "Bed" => "Лежанка",
        "Extras" => "Ещё",
        "Network" => "Сеть",
        "{} (from {})" => "{} (из {})",
        "Hi! I'm visiting from {}" => "Привет! Я в гостях из {}",
        "Network: {} PC(s) nearby, {} pet(s) away" => "Сеть: компьютеров рядом: {}, питомцев в гостях: {}",
        "Send home" => "Отправить домой",
        "Keep a copy of this pet" => "Оставить себе копию питомца",
        "No other PCs found" => "Других компьютеров не найдено",
        "Anywhere" => "Куда угодно",
        "Go visiting" => "Сходить в гости",
        "Give a copy to" => "Подарить копию",
        "Give pets to" => "Подарить питомцев",
        "Give pets to {}" => "Подарить питомцев: {}",
        "Give" => "Подарить",
        "Asking {} if they want {} pet(s)..." => "Спрашиваю {}, нужны ли им питомцы ({})...",
        "{} wants to give you {} pets: {}.\n\nAdd them to your pets?" => "{} хочет подарить вам питомцев ({}): {}.\n\nДобавить их к вашим питомцам?",
        "{} new pets from {}: tray > Choose pets" => "{} новых питомцев от {}: трей > Выбрать питомцев",
        "{} pets" => "{} питомцев",
        "{} new pets now live on {}!" => "{} новых питомцев теперь живут на {}!",
        "Asking {} if they want me..." => "Спрашиваю {}, нужен ли я там...",
        "{} wants to give you a pet: {}.\n\nAdd it to your pets?" => "{} хочет подарить вам питомца: {}.\n\nДобавить его к вашим питомцам?",
        "Couldn't get {} from {}" => "Не удалось получить {} от {}",
        "{} didn't answer, {} stays only here" => "{} не ответил, {} остаётся только здесь",
        "Couldn't add {}: {}" => "Не удалось добавить {}: {}",
        "Hi! {} sent me. I live here now" => "Привет! Меня прислал {}. Теперь я живу здесь",
        "{} now lives on {} too!" => "{} теперь живёт и на {}!",
        "{} already has {}" => "У {} уже есть {}",
        "{} said no thanks to {}" => "{} отказался от {}",
        "Couldn't send {} to {}" => "Не удалось отправить {} на {}",
        "Send with a message" => "Отправить с запиской",
        "Message" => "Записка",
        "Message for {}:" => "Записка для {}:",
        "Message delivered!" => "Записку доставил!",
        "Nobody was there, so I brought your message back" => "Там никого не было, принёс записку обратно",
        "A message from {}!" => "Записка от {}!",
        "Pets walk off your screen and visit other PCs running Popeto on the same network, then come back." => "Питомцы уходят с экрана в гости на другие компьютеры с Popeto в той же сети и потом возвращаются.",
        "Visit other PCs on the local network" => "Ходить в гости на другие компьютеры в локальной сети",
        "Pets go visiting by themselves now and then" => "Питомцы иногда сами уходят в гости",
        "A visit lasts (minutes)" => "Длительность визита (минут)",
        "This PC's name for others" => "Имя этого компьютера для других",
        "Empty = the computer name." => "Пусто = имя компьютера.",
        "Other PCs (addresses)" => "Другие компьютеры (адреса)",
        "Only needed if they aren't found automatically, e.g. 192.168.1.20, office-pc. Comma separated." => "Нужно, только если они не находятся сами, например 192.168.1.20, office-pc. Через запятую.",
        "Visiting pets that you don't have are sent over the network for the visit (kept in the visitors folder). Uses port 47811; Windows may ask to allow Popeto in the firewall. Right click a pet > Go visiting." => "Гостей, которых у вас нет, передают по сети на время визита (хранятся в папке visitors). Порт 47811; Windows может спросить разрешение для Popeto в брандмауэре. Правый клик по питомцу > Сходить в гости.",
        "Timer" => "Таймер",
        "{} min" => "{} мин",
        "Cancel timer ({} min left)" => "Отменить таймер (осталось {} мин)",
        "OK, {} min!" => "Хорошо, {} мин!",
        "Name..." => "Имя...",
        "Pet name" => "Имя питомца",
        "Name for {}:" => "Имя для {}:",
        "Laser pointer (follows the mouse)" => "Лазерная указка (за мышкой)",
        "Chalk (pets draw now and then)" => "Мелки (питомцы иногда рисуют)",
        "Laser pointer" => "Лазерная указка",
        "Chalk" => "Мелки",
        "Reminders: a pet brings you a note. Minutes, 0 = off." => "Напоминания: питомец приносит записку. В минутах, 0 = выключено.",
        "Take a break after (minutes at the computer)" => "Перерыв через (минут за компьютером)",
        "The break timer starts over when you are away for 5 minutes or more." => "Отсчёт перерыва начинается заново, если вы отошли на 5 минут и больше.",
        "Drink water every (minutes)" => "Попить воды каждые (минут)",
        "Kitchen timer: tray menu > Timer. From scripts: Popeto.exe --say \"Build done\" or --note \"text\"." => "Таймер: меню в трее > Таймер. Из скриптов: Popeto.exe --say \"Сборка готова\" или --note \"текст\".",
        "Pets dance when music plays" => "Питомцы танцуют, когда играет музыка",
        "Pets can stand on desktop icons" => "Питомцы могут стоять на значках рабочего стола",
        "Show a pet's name when the mouse is over it (right click a pet > Name...)" => "Показывать имя питомца при наведении мыши (правый клик по питомцу > Имя...)",
        "Remove all toys" => "Убрать все игрушки",
        "What pets do on their own, with each other, your windows and you." => "Что питомцы делают сами: друг с другом, с окнами и с вами.",
        "Pets can stand on each other's heads" => "Питомцы могут стоять друг у друга на головах",
        "Pets greet each other when they meet" => "Питомцы здороваются при встрече",
        "Pets play tag, follow the leader and fetch the ball" => "Питомцы играют в догонялки, ходят паровозиком и приносят мячик",
        "Pets sometimes hide behind the window they climb" => "Питомцы иногда прячутся за окно, по которому лазают",
        "Pets push windows (moves your windows a little)" => "Питомцы толкают окна (немного двигают ваши окна)",
        "Pets fall asleep when you're away" => "Питомцы засыпают, когда вас нет",
        "    ...after this many minutes" => "    ...через столько минут",
        "Greetings in the morning, lunch reminder, sleepy at night" => "Утром здороваются, в обед напоминают поесть, ночью сонные",
        "Little hearts, stars and zzz above pets" => "Сердечки, звёздочки и zzz над питомцами",
        "Pets talk about what they do (speech bubbles)" => "Питомцы говорят о том, что делают (реплики)",
        "Where to find pets (click a link, download in the browser):" => "Где найти питомцев (нажмите на ссылку, скачивайте в браузере):",
        "Big community library, no account needed. Popeto converts its pets automatically." => "Большая библиотека от сообщества, без регистрации. Popeto сам конвертирует её питомцев.",
        "Hundreds of Pokemon and other characters; opens the free ones. Download via the site's ad page." => "Сотни покемонов и других персонажей; открываются бесплатные. Скачивание через страницу сайта с рекламой.",
        "Home of Shimeji-ee and its official pets." => "Дом Shimeji-ee и его официальных питомцев.",
        "Community: new pets, requests and help." => "Сообщество: новые питомцы, просьбы и помощь.",
        "The most pets of all; downloading needs a free account." => "Больше всего питомцев; для скачивания нужен бесплатный аккаунт.",
        "Add downloaded pet: {}" => "Добавить скачанного питомца: {}",
        "New pet in Downloads: {}. Click here to add it." => "В «Загрузках» новый питомец: {}. Нажмите, чтобы добавить.",
        "Offer to add pets downloaded to the Downloads folder" => "Предлагать добавить питомцев, скачанных в «Загрузки»",
        "Pets added: {}" => "Добавлены питомцы: {}",
        "Could not add:\n{}" => "Не удалось добавить:\n{}",
        "Summon" => "Позвать",
        "Random pet" => "Случайного",
        "Everyone: follow cursor" => "Все: бегите за курсором",
        "Restore moved windows ({})" => "Вернуть сдвинутые окна ({})",
        "Restore moved windows" => "Вернуть сдвинутые окна",
        "Dismiss all" => "Убрать всех",
        "Pause" => "Пауза",
        "Pranks" => "Шалости",
        "Options" => "Параметры",
        "Open pets folder" => "Открыть папку питомцев",
        "Edit settings file" => "Открыть файл настроек",
        "Reload pets" => "Перезагрузить питомцев",
        "Exit" => "Выход",
        "Start with Windows" => "Запускать вместе с Windows",
        "Start Popeto when Windows starts" => "Запускать Popeto вместе с Windows",
        "Check for updates (now {})" => "Проверить обновления (сейчас {})",
        "Update to {}" => "Обновить до {}",
        "Popeto {} is out (you have {}).\n\nUpdate now? Popeto restarts by itself; your pets and settings stay." => "Вышел Popeto {} (у вас {}).\n\nОбновить сейчас? Popeto перезапустится сам; питомцы и настройки останутся.",
        "You have the latest version ({})" => "У вас последняя версия ({})",
        "Couldn't check for updates: {}" => "Не удалось проверить обновления: {}",
        "no releases found" => "релизы не найдены",
        "Downloading the update..." => "Скачиваю обновление...",
        "Update failed: {}" => "Обновление не удалось: {}",
        "Updated to {}!" => "Обновился до {}!",
        "Check GitHub for new versions (asks before updating)" => "Проверять новые версии на GitHub (спросит перед обновлением)",
        "Play with windows" => "Играть с окнами",
        "Allow carrying && throwing windows" => "Разрешить таскать и кидать окна",
        "Keep thrown windows on screen" => "Не выкидывать окна за экран",
        "Shake windows on hard landing" => "Трясти окно при жёстком приземлении",
        "Allow cloning" => "Разрешить клонирование",
        "Only one of each pet" => "Только по одному каждого питомца",
        "Only one of each pet (no duplicates)" => "Только по одному питомцу каждого вида (без повторов)",
        "Hide during fullscreen apps" => "Прятаться при полноэкранных приложениях",
        "Sounds" => "Звуки",
        "Size" => "Размер",
        "Opacity" => "Прозрачность",
        "Language" => "Язык",
        "Auto (Windows language)" => "Авто (язык Windows)",
        "Calm" => "Спокойно",
        "Normal" => "Обычно",
        "Chaos" => "Хаос",
        "Bring notes" => "Приносить записки",
        "Bring pictures" => "Приносить картинки",
        "    ...also fresh ones from the internet (not saved)" => "    ...и свежие из интернета (не сохраняются)",
        "Muddy footprints" => "Грязные следы",
        "Grab the cursor (shake mouse to escape)" => "Хватать курсор (потрясите мышь, чтобы вырваться)",
        "Speech bubbles" => "Реплики",
        "Honk" => "Гудеть",
        "Stomp && knock down windows" => "Топтать и ронять окна",
        "How often" => "Как часто",
        "Do a prank now" => "Пошалить сейчас",
        "Take the ball away" => "Забрать мячик",
        "Throw a ball" => "Кинуть мячик",
        "Clean up notes && footprints ({})" => "Убрать записки и следы ({})",
        "Open pranks folder" => "Открыть папку шалостей",
        "Panic key: {}" => "Тревожная кнопка: {}",
        // pet menu
        "Follow cursor" => "Бежать за курсором",
        "Do something" => "Сделать...",
        "Allowed behaviours" => "Разрешённые действия",
        "Another {}" => "Ещё один: {}",
        "Dismiss" => "Убрать",
        "Dismiss others" => "Убрать остальных",
        // settings window
        "Popeto settings" => "Настройки Popeto",
        "General" => "Общие",
        "Windows" => "Окна",
        "Pet size" => "Размер питомцев",
        "Auto (Windows display scaling)" => "Авто (масштаб Windows)",
        "Image smoothing" => "Сглаживание",
        "Smooth" => "Плавное",
        "Sharp pixels (pixel art)" => "Чёткие пиксели (пиксель-арт)",
        "Maximum number of pets" => "Максимум питомцев",
        "Interface language" => "Язык интерфейса",
        "Show the pet chooser every time Popeto starts" => "Показывать выбор питомцев при каждом запуске",
        "Play sounds that come with pets" => "Звуки питомцев",
        "Allow pets to clone themselves" => "Разрешить питомцам клонироваться",
        "Changing size or smoothing reloads the pets (they stay where they are)." => "Смена размера или сглаживания перезагружает питомцев (они остаются на месте).",
        "Pets play with windows (stand, climb, hang on them)" => "Питомцы играют с окнами (стоят, лазают, висят)",
        "Pets may carry and throw windows" => "Питомцы могут таскать и кидать окна",
        "Shake a window when a pet lands on it hard" => "Трясти окно, когда питомец жёстко приземляется",
        "Hide pets while a fullscreen app is in front" => "Прятать питомцев, когда открыто полноэкранное приложение",
        "Still show pets over" => "Но показывать поверх",
        "Fullscreen apps that keep pets visible (exe names). mstsc = Remote Desktop." => "Полноэкранные программы, поверх которых питомцы остаются (имена exe). mstsc = удалённый рабочий стол.",
        "Ignore these windows" => "Не трогать эти окна",
        "Exe names (without .exe) or parts of window titles, comma separated. Pets won't touch them." => "Имена exe (без .exe) или части заголовков окон через запятую. Питомцы их не тронут.",
        "Goose-style mischief. Everything here is off until you tick it." => "Шалости в стиле гуся. Всё выключено, пока не отметите.",
        "Leave muddy footprints" => "Оставлять грязные следы",
        "Grab the mouse cursor (shake the mouse to escape)" => "Хватать курсор (потрясите мышь, чтобы вырваться)",
        "Stomp on windows and knock them down" => "Топтать окна и сворачивать их",
        "Calm - rarely" => "Спокойно - редко",
        "Chaos - all the time" => "Хаос - постоянно",
        "Panic key" => "Тревожная кнопка",
        "Hides every pet and stops pranks; press again to bring them back. E.g. Ctrl+Shift+F12, Ctrl+Alt+P." => "Прячет всех питомцев и останавливает шалости; нажмите ещё раз, чтобы вернуть. Например Ctrl+Shift+F12, Ctrl+Alt+P.",
        "Texts, pictures, sounds: pranks\\universal (any language) + pranks\\{} (this language)." => "Тексты, картинки, звуки: pranks\\universal (для всех языков) + pranks\\{} (этот язык).",
        "Cancel" => "Отмена",
        "Apply" => "Применить",
        // chooser
        "Popeto - choose your pets" => "Popeto - выберите питомцев",
        "Click a pet to switch it on or off. Double-click keeps only that one.    {} of {} selected" => "Щёлкните питомца, чтобы включить или выключить. Двойной щелчок - оставить только его.    Выбрано {} из {}",
        "Select all" => "Выбрать всех",
        "Select none" => "Снять все",
        "Show at startup" => "Показывать при запуске",
        // messages
        "Popeto is already running - look for its icon in the tray." => "Popeto уже запущен - ищите его значок в трее.",
        "No pets found.\n\nPut Shimeji pets into:\n{}\n\nEach pet is a folder with shime*.png images (and usually a conf folder with actions.xml and behaviors.xml).\nDetails are in popeto.log." =>
            "Питомцы не найдены.\n\nПоложите питомцев Shimeji в папку:\n{}\n\nКаждый питомец - это папка с картинками shime*.png (и обычно папкой conf с actions.xml и behaviors.xml).\nПодробности в popeto.log.",
        _ => s,
    }
}
