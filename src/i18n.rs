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
