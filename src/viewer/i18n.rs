//! The atlas in other tongues.
//!
//! The viewer was written in English all the way down: every panel title, every
//! legend caption, every sentence a hover card says. This module is the one place
//! that knows the interface can be read in another language, and it is deliberately
//! small and dependency-free — a table, a lookup, and a language you can pick.
//!
//! # The three rules that keep this honest
//!
//! 1. **English is the key.** Every translatable line is written at its call site
//!    — `caption(ui, self.t("Tint = elevation above / depth below the sea surface."))`
//!    — and this module holds only the *other* languages. A line with no entry
//!    here simply stays English, so the atlas can never show a missing-text marker,
//!    and adding a translation never means touching a call site.
//! 2. **A template is one string.** Lines with figures are written with `{}`
//!    placeholders and filled by [`Language::fill`], so a translation is free to
//!    move a number to wherever its grammar wants it. The tests check that every
//!    translation has exactly as many placeholders as its English key.
//! 3. **English is always complete.** The English column *is* the literals in the
//!    source, so the default atlas reads exactly as it always has — which is what
//!    every existing test asserts.
//!
//! What is deliberately *not* translated: the names the world invents — realms,
//! languages, families, biomes — because they are the world's own data rather
//! than the atlas's voice, and the console output of the headless pipeline, which
//! is for scripts.

use eframe::egui;

/// A language the atlas can be read in.
///
/// The order is the order of the picker: English first — the reference language,
/// and the one the engine's own vocabulary is written in — then the rest by their
/// native names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Language {
    #[default]
    English,
    Spanish,
    French,
    German,
    Portuguese,
    Turkish,
}

impl Language {
    /// Every language, in picker order.
    pub const ALL: [Language; 6] = [
        Language::English,
        Language::Spanish,
        Language::French,
        Language::German,
        Language::Portuguese,
        Language::Turkish,
    ];

    /// The ISO 639-1 code, for the language file and the `--lang` flag.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Spanish => "es",
            Language::French => "fr",
            Language::German => "de",
            Language::Portuguese => "pt",
            Language::Turkish => "tr",
        }
    }

    /// The language's name *in itself*, which is what a picker shows: a reader
    /// looking for their own tongue should recognize it without reading English.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Spanish => "Español",
            Language::French => "Français",
            Language::German => "Deutsch",
            Language::Portuguese => "Português",
            Language::Turkish => "Türkçe",
        }
    }

    /// Parse a language tag — `es`, `es-ES`, `pt_BR`, `tr_TR.UTF-8`.
    ///
    /// Only the part before the first `-`, `_` or `.` matters, which is exactly
    /// what a POSIX locale variable and a BCP 47 tag both lead with. Anything
    /// unknown reads as English: a world atlas that refuses to open is worse than
    /// one in the wrong language.
    pub fn from_tag(tag: &str) -> Self {
        Self::from_known_tag(tag).unwrap_or(Language::English)
    }

    /// Parse a tag, but only if the atlas actually speaks it.
    ///
    /// This is what `--lang` uses: an unsupported code must not silently mean
    /// English and overrule a saved preference.
    pub fn from_known_tag(tag: &str) -> Option<Self> {
        let primary = tag
            .split(['-', '_', '.'])
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|language| language.code() == primary)
    }

    /// The language the desktop is set to, from the POSIX locale variables.
    ///
    /// `LC_ALL` wins over `LC_MESSAGES`, which wins over `LANG` — the same
    /// precedence every other Unix program uses. `C`/`POSIX` fall back to English.
    pub fn from_env() -> Self {
        for name in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(value) = std::env::var(name)
                && !value.is_empty()
            {
                return Self::from_tag(&value);
            }
        }
        Language::English
    }

    /// True for the reference language, whose text needs no lookup.
    pub fn is_english(self) -> bool {
        self == Language::English
    }

    /// Translate one interface line.
    ///
    /// `english` is the line exactly as it is written at the call site; it is
    /// returned untouched for English and for any line this atlas has not been
    /// translated into yet.
    pub fn text(self, english: &'static str) -> &'static str {
        if self.is_english() {
            return english;
        }
        let Some(row) = catalog(english) else {
            return english;
        };
        row[self.column()]
    }

    /// Translate a line with `{}` placeholders and fill them, in order.
    ///
    /// The values are already formatted by the caller (`{:.0} m` is written
    /// `format!("{:.0}", metres)`), so a translation only ever has to move a
    /// value, never re-specify how it is printed.
    pub fn fill(self, template: &'static str, args: &[&dyn std::fmt::Display]) -> String {
        fill_template(self.text(template), args)
    }

    /// This language's index into a catalog row (English is the key itself).
    fn column(self) -> usize {
        match self {
            Language::English => 0,
            Language::Spanish => 0,
            Language::French => 1,
            Language::German => 2,
            Language::Portuguese => 3,
            Language::Turkish => 4,
        }
    }

    /// The saved preference: a file beside the world cache, holding one language
    /// code. Best effort — a missing or unreadable file is simply "no preference".
    pub fn load_preference(dir: &std::path::Path) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join(PREFERENCE_FILE)).ok()?;
        let tag = text.trim();
        Self::ALL
            .into_iter()
            .find(|language| language.code() == tag)
    }

    /// Remember the reader's choice. Errors are ignored: a read-only atlas is
    /// still a usable one.
    pub fn save_preference(self, dir: &std::path::Path) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(dir.join(PREFERENCE_FILE), self.code());
    }
}

/// The language file, kept with the worlds: the only directory the atlas writes.
const PREFERENCE_FILE: &str = "viewer_language.txt";

/// Fill `{}` placeholders in order. Surplus placeholders are left as they are and
/// surplus arguments are ignored, so a mistranslated template can never panic.
fn fill_template(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    for arg in args {
        let Some((before, after)) = rest.split_once("{}") else {
            break;
        };
        out.push_str(before);
        use std::fmt::Write;
        let _ = write!(out, "{arg}");
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The picker, drawn in the toolbar: the globe, and the current language in its
/// own name.
///
/// Returns the language the reader chose, or `None` when they left it alone.
pub fn language_picker(ui: &mut egui::Ui, current: Language) -> Option<Language> {
    let mut chosen = None;
    egui::ComboBox::from_id_salt("language_picker")
        .selected_text(format!("🌐 {}", current.native_name()))
        .width(150.0)
        .show_ui(ui, |ui| {
            for language in Language::ALL {
                if ui
                    .selectable_label(language == current, language.native_name())
                    .clicked()
                {
                    chosen = Some(language);
                }
            }
        });
    chosen
}

// ─── THE CATALOG ─────────────────────────────────────────────────────────────
//
// One row per line: the English key, then Spanish, French, German, Portuguese
// and Turkish. A row is read by `Language::text`; a line with no row stays
// English. Rows are grouped the way the interface is: chrome, the genesis panel,
// the legends, the hover cards, the configuration panel and the world's
// vocabulary.

/// Look one English line up. `None` means "no translation yet" — stay English.
fn catalog(english: &str) -> Option<[&'static str; 5]> {
    let row = match english {
        // ── Chrome: the toolbar ──────────────────────────────────────────────
        "🌍 Atlas Viewer" => [
            "🌍 Atlas del Mundo",
            "🌍 Atlas du Monde",
            "🌍 Atlas-Betrachter",
            "🌍 Atlas do Mundo",
            "🌍 Atlas Görüntüleyici",
        ],
        "🗺 Physical" => ["🗺 Físico", "🗺 Physique", "🗺 Physisch", "🗺 Físico", "🗺 Fiziki"],
        "⛰ Topographic" => [
            "⛰ Topográfico",
            "⛰ Topographique",
            "⛰ Topografisch",
            "⛰ Topográfico",
            "⛰ Topografik",
        ],
        "🌦 Climate" => ["🌦 Clima", "🌦 Climat", "🌦 Klima", "🌦 Clima", "🌦 İklim"],
        "🌡 Temperature" => [
            "🌡 Temperatura",
            "🌡 Température",
            "🌡 Temperatur",
            "🌡 Temperatura",
            "🌡 Sıcaklık",
        ],
        "🪨 Terrain" => ["🪨 Relieve", "🪨 Relief", "🪨 Gelände", "🪨 Relevo", "🪨 Arazi"],
        "🏺 Civilizations" => [
            "🏺 Civilizaciones",
            "🏺 Civilisations",
            "🏺 Zivilisationen",
            "🏺 Civilizações",
            "🏺 Uygarlıklar",
        ],
        "🗣 Languages" => ["🗣 Idiomas", "🗣 Langues", "🗣 Sprachen", "🗣 Línguas", "🗣 Diller"],
        "🏛 Land" => ["🏛 Tierra", "🏛 Terre", "🏛 Land", "🏛 Terra", "🏛 Kara"],
        "🌊 Water" => ["🌊 Agua", "🌊 Eau", "🌊 Wasser", "🌊 Água", "🌊 Su"],
        "🌍 Everywhere" => [
            "🌍 En todas partes",
            "🌍 Partout",
            "🌍 Überall",
            "🌍 Em todo o lado",
            "🌍 Her Yer",
        ],
        "Needs “{}. {}” — {}." => [
            "Requiere «{}. {}» — {}.",
            "Nécessite « {}. {} » — {}.",
            "Benötigt „{}. {}“ — {}.",
            "Requer «{}. {}» — {}.",
            "“{}. {}” gerektirir — {}.",
        ],
        "Tier:" => ["Nivel:", "Niveau :", "Stufe:", "Nível:", "Düzey:"],
        "Provinces" => ["Provincias", "Provinces", "Provinzen", "Províncias", "İller"],
        "Areas" => ["Áreas", "Zones", "Gebiete", "Áreas", "Bölgeler"],
        "Regions" => ["Regiones", "Régions", "Regionen", "Regiões", "Yöreler"],
        "Superregions" => [
            "Superregiones",
            "Superrégions",
            "Superregionen",
            "Super-regiões",
            "Üst yöreler",
        ],
        "Continents" => ["Continentes", "Continents", "Kontinente", "Continentes", "Kıtalar"],
        "Borders" => ["Fronteras", "Frontières", "Grenzen", "Fronteiras", "Sınırlar"],
        "🏺 Civilizations ({})" => [
            "🏺 Civilizaciones ({})",
            "🏺 Civilisations ({})",
            "🏺 Zivilisationen ({})",
            "🏺 Civilizações ({})",
            "🏺 Uygarlıklar ({})",
        ],
        "{}\nAtlas date: {}\nThe overlay tints the provinces each realm holds, on top of whatever this scope already paints." => [
            "{}\nFecha del atlas: {}\nLa superposición tiñe las provincias de cada reino sobre lo que ya pinta este modo.",
            "{}\nDate de l'atlas : {}\nLa superposition teinte les provinces de chaque royaume par-dessus ce que ce mode peint déjà.",
            "{}\nAtlas-Datum: {}\nDie Überlagerung tönt die Provinzen jedes Reiches über dem, was diese Ansicht ohnehin zeigt.",
            "{}\nData do atlas: {}\nA sobreposição tinge as províncias de cada reino por cima do que este modo já pinta.",
            "{}\nAtlas tarihi: {}\nKatman, her krallığın illerini bu görünümün üzerine renklendirir.",
        ],
        "Zoom out" => ["Alejar", "Zoom arrière", "Herauszoomen", "Afastar", "Uzaklaştır"],
        "Zoom in" => ["Acercar", "Zoom avant", "Hineinzoomen", "Aproximar", "Yakınlaştır"],
        "Fit" => ["Ajustar", "Ajuster", "Einpassen", "Ajustar", "Sığdır"],
        "Fit the whole world in the window" => [
            "Ajustar el mundo entero a la ventana",
            "Ajuster le monde entier à la fenêtre",
            "Die ganze Welt ins Fenster einpassen",
            "Ajustar o mundo inteiro à janela",
            "Tüm dünyayı pencereye sığdır",
        ],
        // ── The genesis panel ────────────────────────────────────────────────
        "🧬 Genesis" => ["🧬 Génesis", "🧬 Genèse", "🧬 Genesis", "🧬 Génesis", "🧬 Yaratılış"],
        "A world is grown one act at a time. Watch each one as it happens." => [
            "Un mundo crece acto a acto. Observa cada uno mientras ocurre.",
            "Un monde grandit un acte à la fois. Regardez chacun se dérouler.",
            "Eine Welt wächst Akt für Akt. Beobachten Sie jeden, während er geschieht.",
            "Um mundo cresce ato a ato. Veja cada um a acontecer.",
            "Bir dünya her seferinde bir perde büyür. Her birini olurken izleyin.",
        ],
        "💾 World data" => [
            "💾 Datos del mundo",
            "💾 Données du monde",
            "💾 Weltdaten",
            "💾 Dados do mundo",
            "💾 Dünya verileri",
        ],
        "🪜 The acts  ·  {} of 8 grown" => [
            "🪜 Los actos  ·  {} de 8 completados",
            "🪜 Les actes  ·  {} sur 8 accomplis",
            "🪜 Die Akte  ·  {} von 8 gewachsen",
            "🪜 Os atos  ·  {} de 8 concluídos",
            "🪜 Perdeler  ·  8 perdeden {} tamam",
        ],
        "Seed" => ["Semilla", "Graine", "Startwert", "Semente", "Tohum"],
        "Draw a seed the world has never seen" => [
            "Sortear una semilla que el mundo nunca ha visto",
            "Tirer une graine que le monde n'a jamais vue",
            "Einen Startwert ziehen, den diese Welt noch nie gesehen hat",
            "Sortear uma semente que o mundo nunca viu",
            "Dünyanın hiç görmediği bir tohum çek",
        ],
        "A seed is a whole number — try 42, or press 🎲." => [
            "Una semilla es un número entero: prueba 42 o pulsa 🎲.",
            "Une graine est un nombre entier — essayez 42, ou appuyez sur 🎲.",
            "Ein Startwert ist eine ganze Zahl — versuchen Sie 42, oder drücken Sie 🎲.",
            "Uma semente é um número inteiro — experimente 42 ou prima 🎲.",
            "Tohum bir tam sayıdır — 42 deneyin ya da 🎲 düğmesine basın.",
        ],
        "Tectonics: {} plates · {}% ocean · {} erosion droplets" => [
            "Tectónica: {} placas · {}% de océano · {} gotas de erosión",
            "Tectonique : {} plaques · {} % d'océan · {} gouttes d'érosion",
            "Tektonik: {} Platten · {} % Ozean · {} Erosionstropfen",
            "Tectónica: {} placas · {}% de oceano · {} gotas de erosão",
            "Tektonik: {} levha · %{} okyanus · {} erozyon damlası",
        ],
        "Wait for the act in flight to finish" => [
            "Espera a que termine el acto en curso",
            "Attendez la fin de l'acte en cours",
            "Warten Sie, bis der laufende Akt fertig ist",
            "Aguarde o fim do ato em curso",
            "Süren perdenin bitmesini bekleyin",
        ],
        "A seed is a whole number — Load needs one" => [
            "Una semilla es un número entero: Cargar necesita una",
            "Une graine est un nombre entier — Charger en a besoin",
            "Ein Startwert ist eine ganze Zahl — Laden braucht einen",
            "Uma semente é um número inteiro — Carregar precisa de uma",
            "Tohum bir tam sayıdır — Yükle için gerekli",
        ],
        "There is no cache for this seed yet" => [
            "Todavía no hay caché para esta semilla",
            "Il n'y a pas encore de cache pour cette graine",
            "Für diesen Startwert gibt es noch keinen Cache",
            "Ainda não há cache para esta semente",
            "Bu tohum için henüz önbellek yok",
        ],
        "Grow the world first — there is nothing to save yet" => [
            "Primero genera el mundo: aún no hay nada que guardar",
            "Générez d'abord le monde — il n'y a encore rien à enregistrer",
            "Erzeugen Sie zuerst die Welt — noch gibt es nichts zu speichern",
            "Gere o mundo primeiro — ainda não há nada para guardar",
            "Önce dünyayı oluşturun — henüz kaydedilecek bir şey yok",
        ],
        "📂 Load" => ["📂 Cargar", "📂 Charger", "📂 Laden", "📂 Carregar", "📂 Yükle"],
        "💾 Save" => ["💾 Guardar", "💾 Enregistrer", "💾 Speichern", "💾 Guardar", "💾 Kaydet"],
        "Read {} and pick the panel up where it left off" => [
            "Leer {} y retomar el panel donde se quedó",
            "Lire {} et reprendre le panneau où il s'était arrêté",
            "{} lesen und das Panel dort fortsetzen, wo es aufgehört hat",
            "Ler {} e retomar o painel onde parou",
            "{} okunur ve panel kaldığı yerden sürer",
        ],
        "Read this seed's saved world" => [
            "Leer el mundo guardado de esta semilla",
            "Lire le monde enregistré de cette graine",
            "Die gespeicherte Welt dieses Startwerts lesen",
            "Ler o mundo guardado desta semente",
            "Bu tohumun kayıtlı dünyasını oku",
        ],
        "Write every act grown so far, up to act 6, to saved_seeds/world_<seed>.bin" => [
            "Escribe cada acto ya generado, hasta el acto 6, en saved_seeds/world_<seed>.bin",
            "Écrit chaque acte déjà généré, jusqu'à l'acte 6, dans saved_seeds/world_<seed>.bin",
            "Schreibt jeden bisher gewachsenen Akt bis Akt 6 nach saved_seeds/world_<seed>.bin",
            "Escreve cada ato já gerado, até ao ato 6, em saved_seeds/world_<seed>.bin",
            "Şimdiye kadar oluşan her perdeyi, 6. perdeye kadar saved_seeds/world_<seed>.bin dosyasına yazar",
        ],
        "seed {} · {} · saved {} · Load picks up where it left off" => [
            "semilla {} · {} · guardado {} · Cargar retoma donde se quedó",
            "graine {} · {} · enregistré {} · Charger reprend où cela s'est arrêté",
            "Startwert {} · {} · gespeichert {} · Laden setzt dort fort, wo es aufgehört hat",
            "semente {} · {} · guardado {} · Carregar retoma onde parou",
            "tohum {} · {} · kayıt {} · Yükle kaldığı yerden sürer",
        ],
        "No saved world for seed {} yet — press Save and grow one." => [
            "Aún no hay un mundo guardado para la semilla {}. Pulsa Guardar y genera uno.",
            "Pas encore de monde enregistré pour la graine {} — appuyez sur Enregistrer et générez-en un.",
            "Noch keine gespeicherte Welt für Startwert {} — drücken Sie Speichern und lassen Sie eine wachsen.",
            "Ainda não há um mundo guardado para a semente {} — prima Guardar e gere um.",
            "{} tohumu için henüz kayıtlı dünya yok — Kaydet'e basın ve bir dünya oluşturun.",
        ],
        "A seed is a whole number — Load needs one." => [
            "Una semilla es un número entero: Cargar necesita una.",
            "Une graine est un nombre entier — Charger en a besoin.",
            "Ein Startwert ist eine ganze Zahl — Laden braucht einen.",
            "Uma semente é um número inteiro — Carregar precisa de uma.",
            "Tohum bir tam sayıdır — Yükle için gerekli.",
        ],
        "💾 Save automatically when act 3 and act 6 land" => [
            "💾 Guardar automáticamente al completar los actos 3 y 6",
            "💾 Enregistrer automatiquement à la fin des actes 3 et 6",
            "💾 Automatisch speichern, wenn Akt 3 und Akt 6 fertig sind",
            "💾 Guardar automaticamente ao concluir os atos 3 e 6",
            "💾 3. ve 6. perde tamamlanınca otomatik kaydet",
        ],
        "Auto-save — off: the world is written only when you press Save." => [
            "Guardado automático: desactivado. El mundo solo se escribe al pulsar Guardar.",
            "Enregistrement auto — désactivé : le monde n'est écrit que si vous appuyez sur Enregistrer.",
            "Automatisch speichern — aus: Die Welt wird nur geschrieben, wenn Sie Speichern drücken.",
            "Guardado automático — desligado: o mundo só é escrito quando prime Guardar.",
            "Otomatik kayıt — kapalı: dünya yalnızca Kaydet'e basınca yazılır.",
        ],
        "Auto-save — on: written the moment act 3 lands, and again at act 6, overwriting the earlier save." => [
            "Guardado automático: activado. Se escribe al completar el acto 3 y de nuevo en el acto 6, sobrescribiendo el anterior.",
            "Enregistrement auto — activé : écrit dès la fin de l'acte 3, puis à l'acte 6, en écrasant le précédent.",
            "Automatisch speichern — an: wird geschrieben, sobald Akt 3 fertig ist, und wieder bei Akt 6, wobei der frühere Stand überschrieben wird.",
            "Guardado automático — ligado: é escrito assim que o ato 3 termina, e de novo no ato 6, substituindo o anterior.",
            "Otomatik kayıt — açık: 3. perde biter bitmez, 6. perdede yeniden yazılır ve önceki kaydın üzerine geçer.",
        ],
        "📂 Saved worlds ({})" => [
            "📂 Mundos guardados ({})",
            "📂 Mondes enregistrés ({})",
            "📂 Gespeicherte Welten ({})",
            "📂 Mundos guardados ({})",
            "📂 Kayıtlı dünyalar ({})",
        ],
        "⟳ Rescan" => ["⟳ Reexplorar", "⟳ Rescanner", "⟳ Neu suchen", "⟳ Reexaminar", "⟳ Yeniden tara"],
        "Read saved_seeds again" => [
            "Volver a leer saved_seeds",
            "Relire saved_seeds",
            "saved_seeds erneut lesen",
            "Ler saved_seeds novamente",
            "saved_seeds klasörünü yeniden oku",
        ],
        "Click a world to load it" => [
            "Haz clic en un mundo para cargarlo",
            "Cliquez sur un monde pour le charger",
            "Klicken Sie auf eine Welt, um sie zu laden",
            "Clique num mundo para o carregar",
            "Yüklemek için bir dünyaya tıklayın",
        ],
        "Nothing saved yet. Tick Save above and grow a world, and it will appear here." => [
            "Aún no hay nada guardado. Marca Guardar arriba y genera un mundo, y aparecerá aquí.",
            "Rien n'est encore enregistré. Cochez Enregistrer ci-dessus et générez un monde : il apparaîtra ici.",
            "Noch nichts gespeichert. Setzen Sie oben ein Häkchen bei Speichern und lassen Sie eine Welt wachsen — sie erscheint dann hier.",
            "Ainda nada guardado. Marque Guardar acima e gere um mundo, e ele aparecerá aqui.",
            "Henüz hiçbir şey kaydedilmedi. Yukarıda Kaydet'i işaretleyin ve bir dünya oluşturun; burada görünecek.",
        ],
        "{}   {} · {}  · {}" => [
            "{}   {} · {}  · {}",
            "{}   {} · {}  · {}",
            "{}   {} · {}  · {}",
            "{}   {} · {}  · {}",
            "{}   {} · {}  · {}",
        ],
        "Load seed {} — {} · {} · {}\nPicks the panel up where the save left off." => [
            "Cargar la semilla {} — {} · {} · {}\nRetoma el panel donde lo dejó el guardado.",
            "Charger la graine {} — {} · {} · {}\nReprend le panneau là où l'enregistrement s'est arrêté.",
            "Startwert {} laden — {} · {} · {}\nSetzt das Panel dort fort, wo das Speichern aufgehört hat.",
            "Carregar a semente {} — {} · {} · {}\nRetoma o painel onde o guardado parou.",
            "{} tohumunu yükle — {} · {} · {}\nPaneli kaydın bıraktığı yerden sürdürür.",
        ],
        "acts 1–{}" => ["actos 1–{}", "actes 1–{}", "Akte 1–{}", "atos 1–{}", "perde 1–{}"],
        "a saved world" => [
            "un mundo guardado",
            "un monde enregistré",
            "eine gespeicherte Welt",
            "um mundo guardado",
            "kayıtlı bir dünya",
        ],
        "· loaded" => ["· cargado", "· chargé", "· geladen", "· carregado", "· yüklendi"],
        "· next" => ["· siguiente", "· suivant", "· als Nächstes", "· próximo", "· sıradaki"],
        "· {}s" => ["· {} s", "· {} s", "· {} s", "· {} s", "· {} sn"],
        "Redo from act {} — un-do it and everything after it" => [
            "Rehacer desde el acto {}: deshacerlo a él y a todo lo que le sigue",
            "Refaire depuis l'acte {} — l'annuler ainsi que tout ce qui suit",
            "Ab Akt {} erneut ausführen — ihn und alles danach rückgängig machen",
            "Refazer a partir do ato {} — desfazê-lo e a tudo o que se segue",
            "{} . perdeden yeniden yap — onu ve sonrasındaki her şeyi geri al",
        ],
        "▶▶ Run all remaining" => [
            "▶▶ Ejecutar todos los restantes",
            "▶▶ Exécuter tous les actes restants",
            "▶▶ Alle verbleibenden ausführen",
            "▶▶ Executar todos os restantes",
            "▶▶ Kalanların tümünü çalıştır",
        ],
        "Grow the whole world, act after act, without stopping" => [
            "Genera el mundo entero, acto tras acto, sin detenerte",
            "Générez le monde entier, acte après acte, sans vous arrêter",
            "Lassen Sie die ganze Welt wachsen, Akt für Akt, ohne anzuhalten",
            "Gere o mundo inteiro, ato após ato, sem parar",
            "Dünyanın tamamını, durmadan perde perde oluştur",
        ],
        "⏹ Stop after this act" => [
            "⏹ Detener tras este acto",
            "⏹ Arrêter après cet acte",
            "⏹ Nach diesem Akt anhalten",
            "⏹ Parar após este ato",
            "⏹ Bu perdeden sonra dur",
        ],
        "The queue stops here; this act finishes" => [
            "La cola se detiene aquí; este acto termina",
            "La file s'arrête ici ; cet acte se termine",
            "Die Warteschlange endet hier; dieser Akt läuft zu Ende",
            "A fila para aqui; este ato termina",
            "Sıra burada durur; bu perde tamamlanır",
        ],
        "↺ Reset world" => [
            "↺ Reiniciar mundo",
            "↺ Réinitialiser le monde",
            "↺ Welt zurücksetzen",
            "↺ Reiniciar mundo",
            "↺ Dünyayı sıfırla",
        ],
        "Throw this world away and begin again on the same seed" => [
            "Descarta este mundo y empieza de nuevo con la misma semilla",
            "Abandonnez ce monde et recommencez avec la même graine",
            "Diese Welt verwerfen und mit demselben Startwert neu beginnen",
            "Descarta este mundo e recomeça com a mesma semente",
            "Bu dünyayı at ve aynı tohumla yeniden başla",
        ],
        "starting up" => ["iniciando", "démarrage", "startet", "a iniciar", "başlatılıyor"],
        "⏳ {}. {} — working…" => [
            "⏳ {}. {} — trabajando…",
            "⏳ {}. {} — en cours…",
            "⏳ {}. {} — läuft…",
            "⏳ {}. {} — a trabalhar…",
            "⏳ {}. {} — çalışıyor…",
        ],
        "⚠ {}. {} did not finish — the world is as it was before it started" => [
            "⚠ {}. {} no terminó: el mundo queda como estaba antes de empezar",
            "⚠ {}. {} ne s'est pas terminé — le monde est tel qu'il était avant son début",
            "⚠ {}. {} wurde nicht fertig — die Welt ist wie vor seinem Beginn",
            "⚠ {}. {} não terminou — o mundo está como estava antes de começar",
            "⚠ {}. {} tamamlanamadı — dünya başlamadan önceki hâlinde",
        ],
        "✔ {}. {} — {}s" => [
            "✔ {}. {} — {} s",
            "✔ {}. {} — {} s",
            "✔ {}. {} — {} s",
            "✔ {}. {} — {} s",
            "✔ {}. {} — {} sn",
        ],
        "✔ Adopted the cached world for seed {} — acts 1–{} are already done; act {} is next" => [
            "✔ Mundo en caché adoptado para la semilla {}: los actos 1–{} ya están hechos; el acto {} es el siguiente",
            "✔ Monde en cache adopté pour la graine {} — les actes 1–{} sont déjà faits ; l'acte {} suit",
            "✔ Zwischengespeicherte Welt für Startwert {} übernommen — Akte 1–{} sind bereits fertig; als Nächstes folgt Akt {}",
            "✔ Mundo em cache adotado para a semente {} — os atos 1–{} já estão feitos; o ato {} é o próximo",
            "✔ {} tohumu için önbellekteki dünya alındı — 1–{} perdeleri tamam; sırada {} perde var",
        ],
        "No saved world for seed {} — '{}' is not there. Press Save below, or pick a world from Saved worlds below." => [
            "No hay un mundo guardado para la semilla {}: '{}' no existe. Pulsa Guardar abajo o elige un mundo en Mundos guardados.",
            "Pas de monde enregistré pour la graine {} — '{}' est absent. Appuyez sur Enregistrer ci-dessous, ou choisissez un monde dans Mondes enregistrés.",
            "Keine gespeicherte Welt für Startwert {} — '{}' fehlt. Drücken Sie unten Speichern oder wählen Sie eine Welt unter Gespeicherte Welten.",
            "Não há um mundo guardado para a semente {} — '{}' não existe. Prima Guardar abaixo, ou escolha um mundo em Mundos guardados.",
            "{} tohumu için kayıtlı dünya yok — '{}' bulunamadı. Aşağıdan Kaydet'e basın ya da Kayıtlı dünyalar listesinden seçin.",
        ],
        "'{}' could not be read, so there is nothing to load ({})." => [
            "No se pudo leer '{}', así que no hay nada que cargar ({}).",
            "'{}' n'a pas pu être lu, il n'y a donc rien à charger ({}).",
            "'{}' konnte nicht gelesen werden, es gibt also nichts zu laden ({}).",
            "Não foi possível ler '{}', por isso não há nada para carregar ({}).",
            "'{}' okunamadı, bu yüzden yüklenecek bir şey yok ({}).",
        ],
        "↺ Seed {} cleared — press “{}” to grow it again." => [
            "↺ Semilla {} borrada: pulsa «{}» para generarla de nuevo.",
            "↺ Graine {} effacée — appuyez sur « {} » pour la régénérer.",
            "↺ Startwert {} geleert — drücken Sie „{}“, um sie erneut wachsen zu lassen.",
            "↺ Semente {} apagada — prima «{}» para a gerar de novo.",
            "↺ {} tohumu temizlendi — yeniden oluşturmak için “{}” düğmesine basın.",
        ],
        "↺ Un-done act {} and everything after it — press “{}. {}” to grow it again." => [
            "↺ Deshecho el acto {} y todo lo que le sigue: pulsa «{}. {}» para generarlo de nuevo.",
            "↺ Acte {} et tout ce qui suit annulés — appuyez sur « {}. {} » pour les régénérer.",
            "↺ Akt {} und alles danach rückgängig gemacht — drücken Sie „{}. {}“, um ihn erneut wachsen zu lassen.",
            "↺ Ato {} e tudo o que se segue desfeitos — prima «{}. {}» para o gerar de novo.",
            "↺ {} . perde ve sonrasındaki her şey geri alındı — yeniden oluşturmak için “{}. {}” düğmesine basın.",
        ],
        "🌱 A new world: seed {}. Press “{}” to grow it." => [
            "🌱 Un mundo nuevo: semilla {}. Pulsa «{}» para generarlo.",
            "🌱 Un nouveau monde : graine {}. Appuyez sur « {} » pour le générer.",
            "🌱 Eine neue Welt: Startwert {}. Drücken Sie „{}“, um sie wachsen zu lassen.",
            "🌱 Um mundo novo: semente {}. Prima «{}» para o gerar.",
            "🌱 Yeni bir dünya: tohum {}. Oluşturmak için “{}” düğmesine basın.",
        ],
        "🔀 Seed is now {} — the acts already grown stay; the next act grows from {}." => [
            "🔀 La semilla ahora es {}: los actos ya generados se mantienen; el siguiente acto crecerá desde {}.",
            "🔀 La graine est maintenant {} — les actes déjà générés restent ; l'acte suivant poussera depuis {}.",
            "🔀 Startwert ist jetzt {} — die bereits gewachsenen Akte bleiben; der nächste Akt wächst aus {}.",
            "🔀 A semente agora é {} — os atos já gerados mantêm-se; o próximo ato cresce de {}.",
            "🔀 Tohum artık {} — oluşmuş perdeler kalır; sıradaki perde {} tohumundan büyür.",
        ],
        "✔ Saved acts 1–{} for seed {} to '{}'" => [
            "✔ Guardados los actos 1–{} de la semilla {} en '{}'",
            "✔ Actes 1–{} de la graine {} enregistrés dans '{}'",
            "✔ Akte 1–{} für Startwert {} in '{}' gespeichert",
            "✔ Guardados os atos 1–{} da semente {} em '{}'",
            "✔ {} tohumunun 1–{} perdeleri '{}' dosyasına kaydedildi",
        ],
        "The world could not be saved ({})." => [
            "No se pudo guardar el mundo ({}).",
            "Le monde n'a pas pu être enregistré ({}).",
            "Die Welt konnte nicht gespeichert werden ({}).",
            "Não foi possível guardar o mundo ({}).",
            "Dünya kaydedilemedi ({}).",
        ],
        "at an unknown time" => [
            "en un momento desconocido",
            "à un moment inconnu",
            "zu unbekannter Zeit",
            "num momento desconhecido",
            "bilinmeyen bir zamanda",
        ],
        "just now" => ["ahora mismo", "à l'instant", "gerade eben", "agora mesmo", "az önce"],
        "{} min ago" => ["hace {} min", "il y a {} min", "vor {} min", "há {} min", "{} dk önce"],
        "{} h ago" => ["hace {} h", "il y a {} h", "vor {} h", "há {} h", "{} sa önce"],
        "{} d ago" => ["hace {} d", "il y a {} j", "vor {} T", "há {} d", "{} gün önce"],
        "{} mo ago" => ["hace {} meses", "il y a {} mois", "vor {} Mon.", "há {} meses", "{} ay önce"],
        "{} GB" => ["{} GB", "{} Go", "{} GB", "{} GB", "{} GB"],
        "{} MB" => ["{} MB", "{} Mo", "{} MB", "{} MB", "{} MB"],
        "Growing a world…" => [
            "Generando un mundo…",
            "Un monde pousse…",
            "Eine Welt wächst…",
            "A gerar um mundo…",
            "Bir dünya oluşuyor…",
        ],
        "No world yet" => [
            "Aún no hay mundo",
            "Pas encore de monde",
            "Noch keine Welt",
            "Ainda não há mundo",
            "Henüz dünya yok",
        ],
        "working" => ["trabajando", "en cours", "läuft", "a trabalhar", "çalışıyor"],
        "Seed {}. Every act of creation is a button in the Genesis panel: begin with {}." => [
            "Semilla {}. Cada acto de creación es un botón del panel Génesis: empieza por {}.",
            "Graine {}. Chaque acte de création est un bouton du panneau Genèse : commencez par {}.",
            "Startwert {}. Jeder Schöpfungsakt ist eine Schaltfläche im Genesis-Panel: beginnen Sie mit {}.",
            "Semente {}. Cada ato de criação é um botão no painel Génesis: comece por {}.",
            "Tohum {}. Her yaratılış perdesi Yaratılış panelinde bir düğmedir: {} ile başlayın.",
        ],
        "🔴 {} — {}" => ["🔴 {} — {}", "🔴 {} — {}", "🔴 {} — {}", "🔴 {} — {}", "🔴 {} — {}"],
        "Growing a world — seed {}" => [
            "Generando un mundo: semilla {}",
            "Un monde pousse — graine {}",
            "Eine Welt wächst — Startwert {}",
            "A gerar um mundo — semente {}",
            "Bir dünya oluşuyor — tohum {}",
        ],
        "🔴 live: {} · {}" => [
            "🔴 en vivo: {} · {}",
            "🔴 en direct : {} · {}",
            "🔴 live: {} · {}",
            "🔴 ao vivo: {} · {}",
            "🔴 canlı: {} · {}",
        ],
        // ── The status bar ───────────────────────────────────────────────────
        "🏛 LAND — Continents: {} · Superregions: {} · Regions: {} · Areas: {} · Provinces: {}" => [
            "🏛 TIERRA — Continentes: {} · Superregiones: {} · Regiones: {} · Áreas: {} · Provincias: {}",
            "🏛 TERRES — Continents : {} · Superrégions : {} · Régions : {} · Zones : {} · Provinces : {}",
            "🏛 LAND — Kontinente: {} · Superregionen: {} · Regionen: {} · Gebiete: {} · Provinzen: {}",
            "🏛 TERRA — Continentes: {} · Super-regiões: {} · Regiões: {} · Áreas: {} · Províncias: {}",
            "🏛 KARA — Kıtalar: {} · Üst yöreler: {} · Yöreler: {} · Bölgeler: {} · İller: {}",
        ],
        "🌊 OCEAN — Basins: {} · Superregions: {} · Regions: {} · Areas: {} · Provinces: {}" => [
            "🌊 OCÉANO — Cuencas: {} · Superregiones: {} · Regiones: {} · Áreas: {} · Provincias: {}",
            "🌊 OCÉAN — Bassins : {} · Superrégions : {} · Régions : {} · Zones : {} · Provinces : {}",
            "🌊 OZEAN — Becken: {} · Superregionen: {} · Regionen: {} · Gebiete: {} · Provinzen: {}",
            "🌊 OCEANO — Bacias: {} · Super-regiões: {} · Regiões: {} · Áreas: {} · Províncias: {}",
            "🌊 OKYANUS — Havzalar: {} · Üst yöreler: {} · Yöreler: {} · Bölgeler: {} · İller: {}",
        ],
        "🏺 CIVILIZATIONS — {} · Atlas date: {} · {} · {} realms · {} civilized provinces · population {} · {} people/km²" => [
            "🏺 CIVILIZACIONES — {} · Fecha del atlas: {} · {} · {} reinos · {} provincias civilizadas · población {} · {} hab./km²",
            "🏺 CIVILISATIONS — {} · Date de l'atlas : {} · {} · {} royaumes · {} provinces civilisées · population {} · {} hab./km²",
            "🏺 ZIVILISATIONEN — {} · Atlas-Datum: {} · {} · {} Reiche · {} zivilisierte Provinzen · Bevölkerung {} · {} Einw./km²",
            "🏺 CIVILIZAÇÕES — {} · Data do atlas: {} · {} · {} reinos · {} províncias civilizadas · população {} · {} hab./km²",
            "🏺 UYGARLIKLAR — {} · Atlas tarihi: {} · {} · {} krallık · {} uygar il · nüfus {} · {} kişi/km²",
        ],
        // ── The legends ──────────────────────────────────────────────────────
        "⛰ Topographic Scale" => [
            "⛰ Escala topográfica",
            "⛰ Échelle topographique",
            "⛰ Topografische Skala",
            "⛰ Escala topográfica",
            "⛰ Topografik Ölçek",
        ],
        "Tint = elevation above / depth below the sea surface." => [
            "El tono indica la altitud sobre el mar o la profundidad bajo él.",
            "La teinte indique l'altitude au-dessus ou la profondeur sous la mer.",
            "Der Farbton zeigt Höhe über bzw. Tiefe unter dem Meeresspiegel.",
            "O tom indica a altitude acima ou a profundidade abaixo do mar.",
            "Renk, deniz yüzeyinin üstündeki yüksekliği ya da altındaki derinliği gösterir.",
        ],
        "🗻 Highest peak   +{} m" => [
            "🗻 Cumbre más alta   +{} m",
            "🗻 Plus haut sommet   +{} m",
            "🗻 Höchster Gipfel   +{} m",
            "🗻 Pico mais alto   +{} m",
            "🗻 En yüksek zirve   +{} m",
        ],
        "{}\nClick to fly to the summit" => [
            "{}\nHaz clic para volar a la cumbre",
            "{}\nCliquez pour voler vers le sommet",
            "{}\nKlicken Sie, um zum Gipfel zu fliegen",
            "{}\nClique para voar até ao cume",
            "{}\nZirveye uçmak için tıklayın",
        ],
        "🌊 Deepest trench  {} m" => [
            "🌊 Fosa más profunda  {} m",
            "🌊 Plus profonde fosse  {} m",
            "🌊 Tiefster Graben  {} m",
            "🌊 Fossa mais funda  {} m",
            "🌊 En derin çukur  {} m",
        ],
        "{}\nClick to fly to the deepest point" => [
            "{}\nHaz clic para volar al punto más profundo",
            "{}\nCliquez pour voler vers le point le plus profond",
            "{}\nKlicken Sie, um zum tiefsten Punkt zu fliegen",
            "{}\nClique para voar até ao ponto mais fundo",
            "{}\nEn derin noktaya uçmak için tıklayın",
        ],
        "🌍 Surface: {}% ocean" => [
            "🌍 Superficie: {}% de océano",
            "🌍 Surface : {} % d'océan",
            "🌍 Oberfläche: {} % Ozean",
            "🌍 Superfície: {}% de oceano",
            "🌍 Yüzey: %{} okyanus",
        ],
        "Click a figure above to centre the map on it." => [
            "Haz clic en una cifra de arriba para centrar el mapa en ella.",
            "Cliquez sur un chiffre ci-dessus pour centrer la carte dessus.",
            "Klicken Sie oben auf eine Zahl, um die Karte darauf zu zentrieren.",
            "Clique numa cifra acima para centrar o mapa nela.",
            "Haritayı ortalayacağınız bir sayıya yukarıdan tıklayın.",
        ],
        "🌦 Climate Zones" => [
            "🌦 Zonas climáticas",
            "🌦 Zones climatiques",
            "🌦 Klimazonen",
            "🌦 Zonas climáticas",
            "🌦 İklim Bölgeleri",
        ],
        "Land is classified by mean temperature, annual rainfall and altitude; the sea by its surface temperature." => [
            "La tierra se clasifica por temperatura media, lluvia anual y altitud; el mar, por su temperatura superficial.",
            "La terre est classée selon la température moyenne, les précipitations annuelles et l'altitude ; la mer selon sa température de surface.",
            "Land wird nach Mitteltemperatur, Jahresniederschlag und Höhenlage klassifiziert, das Meer nach seiner Oberflächentemperatur.",
            "A terra é classificada pela temperatura média, precipitação anual e altitude; o mar pela sua temperatura à superfície.",
            "Kara, ortalama sıcaklık, yıllık yağış ve yüksekliğe göre; deniz ise yüzey sıcaklığına göre sınıflandırılır.",
        ],
        "Frozen sea" => ["Mar helado", "Mer gelée", "Gefrorenes Meer", "Mar gelado", "Donmuş deniz"],
        "Polar sea" => ["Mar polar", "Mer polaire", "Polarmeer", "Mar polar", "Kutup denizi"],
        "Cold temperate sea" => [
            "Mar templado frío",
            "Mer tempérée froide",
            "Kaltgemäßigtes Meer",
            "Mar temperado frio",
            "Soğuk ılıman deniz",
        ],
        "Warm temperate sea" => [
            "Mar templado cálido",
            "Mer tempérée chaude",
            "Warmgemäßigtes Meer",
            "Mar temperado quente",
            "Ilık ılıman deniz",
        ],
        "Tropical sea" => ["Mar tropical", "Mer tropicale", "Tropisches Meer", "Mar tropical", "Tropik deniz"],
        "Equatorial sea" => [
            "Mar ecuatorial",
            "Mer équatoriale",
            "Äquatoriales Meer",
            "Mar equatorial",
            "Ekvator denizi",
        ],
        "Mean annual surface temperature: land station means, sea-surface means over the ocean." => [
            "Temperatura superficial media anual: medias de estaciones en tierra y de la superficie del mar.",
            "Température de surface moyenne annuelle : moyennes des stations terrestres, moyennes de surface en mer.",
            "Mittlere Jahres-Oberflächentemperatur: Stationsmittel über Land, Oberflächenmittel über dem Meer.",
            "Temperatura média anual à superfície: médias de estações em terra, médias da superfície do mar.",
            "Yıllık ortalama yüzey sıcaklığı: karada istasyon ortalamaları, denizde yüzey ortalamaları.",
        ],
        "A thin dark line marks every 5 °C isotherm." => [
            "Una fina línea oscura marca cada isoterma de 5 °C.",
            "Une fine ligne sombre marque chaque isotherme de 5 °C.",
            "Eine dünne dunkle Linie markiert jede 5-°C-Isotherme.",
            "Uma linha escura fina marca cada isoterma de 5 °C.",
            "Her 5 °C izotermi ince koyu bir çizgiyle işaretlenir.",
        ],
        "🪨 Terrain Types" => [
            "🪨 Tipos de relieve",
            "🪨 Types de relief",
            "🪨 Geländeformen",
            "🪨 Tipos de relevo",
            "🪨 Arazi Türleri",
        ],
        "Landform bands from the vertical datum; the ocean floor is classified by depth." => [
            "Franjas de relieve desde el datum vertical; el fondo marino se clasifica por profundidad.",
            "Bandes de relief depuis le datum vertical ; le fond marin est classé par profondeur.",
            "Reliefbänder ab dem vertikalen Datum; der Meeresboden wird nach Tiefe klassifiziert.",
            "Faixas de relevo desde o datum vertical; o fundo do mar é classificado por profundidade.",
            "Düşey datumdan itibaren yer şekli kuşakları; okyanus tabanı derinliğe göre sınıflandırılır.",
        ],
        "🏺 The Dawn of Civilization" => [
            "🏺 El amanecer de la civilización",
            "🏺 L'aube de la civilisation",
            "🏺 Die Morgenröte der Zivilisation",
            "🏺 O alvor da civilização",
            "🏺 Uygarlığın Şafağı",
        ],
        "No civilization rose on this world." => [
            "Ninguna civilización surgió en este mundo.",
            "Aucune civilisation n'a surgi sur ce monde.",
            "Auf dieser Welt ist keine Zivilisation entstanden.",
            "Nenhuma civilização surgiu neste mundo.",
            "Bu dünyada hiçbir uygarlık doğmadı.",
        ],
        "The history stage only runs when it is asked for: pass --civs (it is on automatically with --viewer and --plates)." => [
            "La etapa de historia solo se ejecuta cuando se pide: usa --civs (se activa automáticamente con --viewer y --plates).",
            "L'étape d'histoire ne s'exécute que si on la demande : passez --civs (activée automatiquement avec --viewer et --plates).",
            "Der Geschichtsakt läuft nur auf Anforderung: verwenden Sie --civs (bei --viewer und --plates automatisch an).",
            "A etapa de história só corre quando é pedida: use --civs (ativa automaticamente com --viewer e --plates).",
            "Tarih perdesi yalnızca istendiğinde çalışır: --civs kullanın (--viewer ve --plates ile otomatik açılır).",
        ],
        "Atlas date: {} · {}" => [
            "Fecha del atlas: {} · {}",
            "Date de l'atlas : {} · {}",
            "Atlas-Datum: {} · {}",
            "Data do atlas: {} · {}",
            "Atlas tarihi: {} · {}",
        ],
        "🏛 Fly to the Common Era site" => [
            "🏛 Volar al lugar de la Era Común",
            "🏛 Voler vers le site de l'ère commune",
            "🏛 Zum Ort der Common Era fliegen",
            "🏛 Voar até ao local da Era Comum",
            "🏛 Ortak Çağ alanına uç",
        ],
        "The province the world's calendar is reckoned from" => [
            "La provincia desde la que se cuenta el calendario del mundo",
            "La province à partir de laquelle le calendrier du monde est compté",
            "Die Provinz, von der aus der Kalender der Welt gerechnet wird",
            "A província a partir da qual o calendário do mundo é contado",
            "Dünyanın takviminin hesaplandığı il",
        ],
        "{} societies on {} inhabited provinces" => [
            "{} sociedades en {} provincias habitadas",
            "{} sociétés sur {} provinces habitées",
            "{} Gesellschaften auf {} bewohnten Provinzen",
            "{} sociedades em {} províncias habitadas",
            "{} ilde {} toplum",
        ],
        "{} km² inhabited · world population {}" => [
            "{} km² habitados · población mundial {}",
            "{} km² habités · population mondiale {}",
            "{} km² bewohnt · Weltbevölkerung {}",
            "{} km² habitados · população mundial {}",
            "{} km² yerleşim · dünya nüfusu {}",
        ],
        "Mean density {} people/km²" => [
            "Densidad media {} hab./km²",
            "Densité moyenne {} hab./km²",
            "Mittlere Dichte {} Einw./km²",
            "Densidade média {} hab./km²",
            "Ortalama yoğunluk {} kişi/km²",
        ],
        "How the world is organised" => [
            "Cómo está organizado el mundo",
            "Comment le monde est organisé",
            "Wie die Welt gegliedert ist",
            "Como o mundo está organizado",
            "Dünya nasıl örgütlenmiş",
        ],
        "Click a society to fly to its heartland." => [
            "Haz clic en una sociedad para volar a su corazón.",
            "Cliquez sur une société pour voler vers son cœur.",
            "Klicken Sie auf eine Gesellschaft, um zu ihrem Kernland zu fliegen.",
            "Clique em cada sociedade para voar até ao seu coração.",
            "Bir topluma tıklayarak onun kalbine uçun.",
        ],
        "{} ({})" => ["{} ({})", "{} ({})", "{} ({})", "{} ({})", "{} ({})"],
        "Click to fly to this society's heartland" => [
            "Haz clic para volar al corazón de esta sociedad",
            "Cliquez pour voler vers le cœur de cette société",
            "Klicken Sie, um zum Kernland dieser Gesellschaft zu fliegen",
            "Clique para voar até ao coração desta sociedade",
            "Bu toplumun kalbine uçmak için tıklayın",
        ],
        "{} — {} societies · {}% of the land" => [
            "{} — {} sociedades · {}% de la tierra",
            "{} — {} sociétés · {} % des terres",
            "{} — {} Gesellschaften · {} % des Landes",
            "{} — {} sociedades · {}% da terra",
            "{} — {} toplum · arazinin %{} kadarı",
        ],
        "… and {} more, all on the map." => [
            "… y {} más, todos en el mapa.",
            "… et {} de plus, tous sur la carte.",
            "… und {} weitere, alle auf der Karte.",
            "… e mais {}, todos no mapa.",
            "… ve {} tane daha, hepsi haritada.",
        ],
        "🗣 Languages of the World" => [
            "🗣 Idiomas del mundo",
            "🗣 Langues du monde",
            "🗣 Sprachen der Welt",
            "🗣 Línguas do mundo",
            "🗣 Dünya Dilleri",
        ],
        "Each family is a colour, each language a shade of it. A family holds the provinces its speakers settled, from its Urheimat outward." => [
            "Cada familia es un color y cada idioma un tono suyo. Una familia abarca las provincias donde se asentaron sus hablantes, desde su Urheimat hacia fuera.",
            "Chaque famille est une couleur, chaque langue une nuance de celle-ci. Une famille occupe les provinces où ses locuteurs se sont installés, depuis son Urheimat.",
            "Jede Familie ist eine Farbe, jede Sprache ein Farbton davon. Eine Familie umfasst die Provinzen, in denen sich ihre Sprecher niederließen, von ihrer Urheimat aus.",
            "Cada família é uma cor e cada língua um tom dela. Uma família reúne as províncias onde os seus falantes se fixaram, desde a sua Urheimat para fora.",
            "Her dil ailesi bir renk, her dil o rengin bir tonudur. Bir aile, konuşurlarının yerleştiği illeri Urheimat'ından dışarıya doğru kapsar.",
        ],
        "No tongue has been given to this world yet." => [
            "Este mundo aún no tiene lenguas.",
            "Aucune langue n'a encore été donnée à ce monde.",
            "Dieser Welt wurde noch keine Sprache gegeben.",
            "Este mundo ainda não recebeu nenhuma língua.",
            "Bu dünyaya henüz hiçbir dil verilmedi.",
        ],
        "{} languages · {} provinces · {} people" => [
            "{} idiomas · {} provincias · {} personas",
            "{} langues · {} provinces · {} habitants",
            "{} Sprachen · {} Provinzen · {} Menschen",
            "{} línguas · {} províncias · {} pessoas",
            "{} dil · {} il · {} kişi",
        ],
        "Urheimat: {}" => ["Urheimat: {}", "Urheimat : {}", "Urheimat: {}", "Urheimat: {}", "Urheimat: {}"],
        "{} provinces" => ["{} provincias", "{} provinces", "{} Provinzen", "{} províncias", "{} il"],
        // ── The hover cards ──────────────────────────────────────────────────
        "💧 Inland Freshwater Lake" => [
            "💧 Lago interior de agua dulce",
            "💧 Lac d'eau douce intérieur",
            "💧 Binnen-Süßwassersee",
            "💧 Lago interior de água doce",
            "💧 İç Tatlı Su Gölü",
        ],
        "No realm claims the water itself." => [
            "Ningún reino reclama el agua en sí.",
            "Aucun royaume ne revendique l'eau elle-même.",
            "Kein Reich beansprucht das Wasser selbst.",
            "Nenhum reino reclama a água em si.",
            "Suyun kendisini hiçbir krallık talep etmez.",
        ],
        "Lake Surface: {} m" => [
            "Superficie del lago: {} m",
            "Surface du lac : {} m",
            "Seespiegel: {} m",
            "Superfície do lago: {} m",
            "Göl yüzeyi: {} m",
        ],
        "Coordinates: ({}, {})" => [
            "Coordenadas: ({}, {})",
            "Coordonnées : ({}, {})",
            "Koordinaten: ({}, {})",
            "Coordenadas: ({}, {})",
            "Koordinatlar: ({}, {})",
        ],
        "🌊 Uncivilized Sea" => [
            "🌊 Mar sin civilizar",
            "🌊 Mer non civilisée",
            "🌊 Unzivilisiertes Meer",
            "🌊 Mar não civilizado",
            "🌊 Uygarlıksız Deniz",
        ],
        "Depth: {} m" => ["Profundidad: {} m", "Profondeur : {} m", "Tiefe: {} m", "Profundidade: {} m", "Derinlik: {} m"],
        "🏔️ Wasteland Beyond History" => [
            "🏔️ Yermo más allá de la historia",
            "🏔️ Terre désolée hors de l'histoire",
            "🏔️ Ödland jenseits der Geschichte",
            "🏔️ Ermo para lá da história",
            "🏔️ Tarihin Ötesindeki Çoraklık",
        ],
        "Impassable or unclaimed wilderness" => [
            "Naturaleza intransitable o no reclamada",
            "Nature sauvage infranchissable ou non revendiquée",
            "Unpassierbare oder beanspruchte Wildnis",
            "Natureza selvagem intransitável ou não reclamada",
            "Geçilemez ya da sahipsiz vahşi doğa",
        ],
        "Environment: {}" => [
            "Entorno: {}",
            "Environnement : {}",
            "Umgebung: {}",
            "Ambiente: {}",
            "Çevre: {}",
        ],
        "Elevation: {} m" => [
            "Altitud: {} m",
            "Altitude : {} m",
            "Höhe: {} m",
            "Altitude: {} m",
            "Yükseklik: {} m",
        ],
        "🌾 Unclaimed Land" => [
            "🌾 Tierra sin reclamar",
            "🌾 Terre non revendiquée",
            "🌾 Unbeanspruchtes Land",
            "🌾 Terra não reclamada",
            "🌾 Sahipsiz Toprak",
        ],
        "No civilization has risen here" => [
            "Aquí no ha surgido ninguna civilización",
            "Aucune civilisation ne s'est élevée ici",
            "Hier ist keine Zivilisation entstanden",
            "Aqui não surgiu nenhuma civilização",
            "Burada hiçbir uygarlık doğmadı",
        ],
        "Fertile enough to live on, but never the best ground in reach: nothing here outperformed the cradles." => [
            "Suficientemente fértil para vivir, pero nunca la mejor tierra al alcance: nada aquí superó a las cunas.",
            "Assez fertile pour y vivre, mais jamais la meilleure terre à portée : rien ici n'a surpassé les berceaux.",
            "Fruchtbar genug zum Leben, aber nie der beste erreichbare Boden: Nichts hier übertraf die Wiegen.",
            "Fértil o bastante para viver, mas nunca a melhor terra ao alcance: nada aqui superou os berços.",
            "Yaşamaya yetecek kadar verimli, ama erişilebilir en iyi toprak değil: burada hiçbir şey beşikleri geçemedi.",
        ],
        "🌊 Open Water" => ["🌊 Aguas abiertas", "🌊 Pleine mer", "🌊 Offenes Wasser", "🌊 Águas abertas", "🌊 Açık Su"],
        "No land, and so no tongue of its own." => [
            "No hay tierra y, por tanto, ninguna lengua propia.",
            "Pas de terre, donc pas de langue propre.",
            "Kein Land und daher keine eigene Sprache.",
            "Não há terra e, por isso, nenhuma língua própria.",
            "Kara yok, dolayısıyla kendine ait bir dil de yok.",
        ],
        "The water itself belongs to no language." => [
            "El agua en sí no pertenece a ninguna lengua.",
            "L'eau elle-même n'appartient à aucune langue.",
            "Das Wasser selbst gehört keiner Sprache.",
            "A água em si não pertence a nenhuma língua.",
            "Suyun kendisi hiçbir dile ait değildir.",
        ],
        "Language: {}" => ["Idioma: {}", "Langue : {}", "Sprache: {}", "Língua: {}", "Dil: {}"],
        "Autonym: {}" => ["Endónimo: {}", "Autonyme : {}", "Autonym: {}", "Autónimo: {}", "Öz ad: {}"],
        "Province: {}" => ["Provincia: {}", "Province : {}", "Provinz: {}", "Província: {}", "İl: {}"],
        "Society: {}" => ["Sociedad: {}", "Société : {}", "Gesellschaft: {}", "Sociedade: {}", "Toplum: {}"],
        "Size: {} px" => ["Tamaño: {} px", "Taille : {} px", "Größe: {} px", "Tamanho: {} px", "Boyut: {} px"],
        "🏔️ Beyond the Tongues" => [
            "🏔️ Más allá de las lenguas",
            "🏔️ Au-delà des langues",
            "🏔️ Jenseits der Sprachen",
            "🏔️ Para lá das línguas",
            "🏔️ Dillerin Ötesinde",
        ],
        "No language has settled this ground" => [
            "Ninguna lengua se ha asentado en esta tierra",
            "Aucune langue ne s'est installée sur cette terre",
            "Keine Sprache hat sich auf diesem Boden niedergelassen",
            "Nenhuma língua se fixou nesta terra",
            "Bu toprağa hiçbir dil yerleşmedi",
        ],
        "Open Ocean" => ["Océano abierto", "Haute mer", "Offener Ozean", "Oceano aberto", "Açık Okyanus"],
        "Inland Freshwater Lake" => [
            "Lago interior de agua dulce",
            "Lac d'eau douce intérieur",
            "Binnen-Süßwassersee",
            "Lago interior de água doce",
            "İç Tatlı Su Gölü",
        ],
        "Ice-capped Land" => [
            "Tierra cubierta de hielo",
            "Terre couverte de glace",
            "Eisbedecktes Land",
            "Terra coberta de gelo",
            "Buzla Kaplı Kara",
        ],
        "⛰ {}" => ["⛰ {}", "⛰ {}", "⛰ {}", "⛰ {}", "⛰ {}"],
        "Elevation: {} m above sea level" => [
            "Altitud: {} m sobre el nivel del mar",
            "Altitude : {} m au-dessus du niveau de la mer",
            "Höhe: {} m über dem Meeresspiegel",
            "Altitude: {} m acima do nível do mar",
            "Yükseklik: deniz seviyesinden {} m yukarıda",
        ],
        "Depth: {} m below sea level" => [
            "Profundidad: {} m bajo el nivel del mar",
            "Profondeur : {} m sous le niveau de la mer",
            "Tiefe: {} m unter dem Meeresspiegel",
            "Profundidade: {} m abaixo do nível do mar",
            "Derinlik: deniz seviyesinin {} m altında",
        ],
        "Relief band: {}  ({})" => [
            "Franja de relieve: {}  ({})",
            "Bande de relief : {}  ({})",
            "Reliefband: {}  ({})",
            "Faixa de relevo: {}  ({})",
            "Yer şekli kuşağı: {}  ({})",
        ],
        "Sea-Surface Temperature: {} °C" => [
            "Temperatura de la superficie del mar: {} °C",
            "Température de surface de la mer : {} °C",
            "Meeresoberflächentemperatur: {} °C",
            "Temperatura da superfície do mar: {} °C",
            "Deniz yüzeyi sıcaklığı: {} °C",
        ],
        "Temperature: {} °C" => [
            "Temperatura: {} °C",
            "Température : {} °C",
            "Temperatur: {} °C",
            "Temperatura: {} °C",
            "Sıcaklık: {} °C",
        ],
        "Precipitation: {} mm/yr" => [
            "Precipitación: {} mm/año",
            "Précipitations : {} mm/an",
            "Niederschlag: {} mm/Jahr",
            "Precipitação: {} mm/ano",
            "Yağış: {} mm/yıl",
        ],
        "River Flow: {} m³/s" => [
            "Caudal: {} m³/s",
            "Débit fluvial : {} m³/s",
            "Abfluss: {} m³/s",
            "Caudal: {} m³/s",
            "Nehir debisi: {} m³/s",
        ],
        "🌦 Inland Lake" => [
            "🌦 Lago interior",
            "🌦 Lac intérieur",
            "🌦 Binnensee",
            "🌦 Lago interior",
            "🌦 İç Göl",
        ],
        "Fresh water, read with the climate of its shores." => [
            "Agua dulce, leída con el clima de sus orillas.",
            "Eau douce, lue avec le climat de ses rives.",
            "Süßwasser, gelesen mit dem Klima seiner Ufer.",
            "Água doce, lida com o clima das suas margens.",
            "Tatlı su, kıyılarının iklimiyle okunur.",
        ],
        "🌦 {}" => ["🌦 {}", "🌦 {}", "🌦 {}", "🌦 {}", "🌦 {}"],
        "Sea-surface temperature: {} °C" => [
            "Temperatura de la superficie del mar: {} °C",
            "Température de surface : {} °C",
            "Oberflächentemperatur: {} °C",
            "Temperatura à superfície do mar: {} °C",
            "Deniz yüzeyi sıcaklığı: {} °C",
        ],
        "Perennial sea ice." => [
            "Hielo marino permanente.",
            "Glace de mer pérenne.",
            "Mehrjähriges Meereis.",
            "Gelo marinho permanente.",
            "Kalıcı deniz buzu.",
        ],
        "Mean temperature: {} °C" => [
            "Temperatura media: {} °C",
            "Température moyenne : {} °C",
            "Mitteltemperatur: {} °C",
            "Temperatura média: {} °C",
            "Ortalama sıcaklık: {} °C",
        ],
        "Annual rainfall: {} mm/yr" => [
            "Lluvia anual: {} mm/año",
            "Précipitations annuelles : {} mm/an",
            "Jahresniederschlag: {} mm/Jahr",
            "Precipitação anual: {} mm/ano",
            "Yıllık yağış: {} mm/yıl",
        ],
        "Sea ice" => ["Hielo marino", "Glace de mer", "Meereis", "Gelo marinho", "Deniz buzu"],
        "Sea surface" => [
            "Superficie del mar",
            "Surface de la mer",
            "Meeresoberfläche",
            "Superfície do mar",
            "Deniz yüzeyi",
        ],
        "Lake surface" => [
            "Superficie del lago",
            "Surface du lac",
            "Seespiegel",
            "Superfície do lago",
            "Göl yüzeyi",
        ],
        "Ice-capped land" => [
            "Tierra cubierta de hielo",
            "Terre couverte de glace",
            "Eisbedecktes Land",
            "Terra coberta de gelo",
            "Buzla kaplı kara",
        ],
        "Land surface" => [
            "Superficie terrestre",
            "Surface terrestre",
            "Landoberfläche",
            "Superfície terrestre",
            "Kara yüzeyi",
        ],
        "🌡 {} °C" => ["🌡 {} °C", "🌡 {} °C", "🌡 {} °C", "🌡 {} °C", "🌡 {} °C"],
        "Isotherm band: {}" => [
            "Banda isotérmica: {}",
            "Bande isotherme : {}",
            "Isothermenband: {}",
            "Faixa isotérmica: {}",
            "İzoterm kuşağı: {}",
        ],
        "🪨 {}" => ["🪨 {}", "🪨 {}", "🪨 {}", "🪨 {}", "🪨 {}"],
        "Band: {}" => ["Banda: {}", "Bande : {}", "Band: {}", "Faixa: {}", "Kuşak: {}"],
        "Ground: {} ({} m rise per pixel)" => [
            "Terreno: {} ({} m de desnivel por píxel)",
            "Sol : {} ({} m de dénivelé par pixel)",
            "Gelände: {} ({} m Anstieg pro Pixel)",
            "Terreno: {} ({} m de desnível por píxel)",
            "Zemin: {} (piksel başına {} m yükseliş)",
        ],
        "🌍 {}" => ["🌍 {}", "🌍 {}", "🌍 {}", "🌍 {}", "🌍 {}"],
        "🧊 Ocean Province #{}" => [
            "🧊 Provincia oceánica #{}",
            "🧊 Province océanique n° {}",
            "🧊 Ozeanprovinz #{}",
            "🧊 Província oceânica #{}",
            "🧊 Okyanus İli #{}",
        ],
        "{} · Impassable Polar Wasteland" => [
            "{} · yermo polar intransitable",
            "{} · terre désolée polaire infranchissable",
            "{} · unpassierbares Polarödland",
            "{} · ermo polar intransitável",
            "{} · geçilemez kutup çoraklığı",
        ],
        "🌊 Ocean Province #{}" => [
            "🌊 Provincia oceánica #{}",
            "🌊 Province océanique n° {}",
            "🌊 Ozeanprovinz #{}",
            "🌊 Província oceânica #{}",
            "🌊 Okyanus İli #{}",
        ],
        "Area:" => ["Área:", "Zone :", "Gebiet:", "Área:", "Bölge:"],
        "Region:" => ["Región:", "Région :", "Region:", "Região:", "Yöre:"],
        "Superregion:" => [
            "Superregión:",
            "Superrégion :",
            "Superregion:",
            "Super-região:",
            "Üst yöre:",
        ],
        "Ocean Basin:" => [
            "Cuenca oceánica:",
            "Bassin océanique :",
            "Ozeanbecken:",
            "Bacia oceânica:",
            "Okyanus havzası:",
        ],
        "Continent:" => ["Continente:", "Continent :", "Kontinent:", "Continente:", "Kıta:"],
        "Anchor Coast:" => [
            "Costa de anclaje:",
            "Côte d'ancrage :",
            "Ankerküste:",
            "Costa de ancoragem:",
            "Bağlantı kıyısı:",
        ],
        "Province #{}" => ["Provincia #{}", "Province n° {}", "Provinz #{}", "Província #{}", "İl #{}"],
        "Sea Floor Depth: {} m" => [
            "Profundidad del fondo marino: {} m",
            "Profondeur du fond marin : {} m",
            "Meeresboden-Tiefe: {} m",
            "Profundidade do fundo marinho: {} m",
            "Deniz tabanı derinliği: {} m",
        ],
        "🧊 Polar Ice Wasteland" => [
            "🧊 Yermo de hielo polar",
            "🧊 Désert de glace polaire",
            "🧊 Polares Eisödland",
            "🧊 Ermo de gelo polar",
            "🧊 Kutup Buzu Çoraklığı",
        ],
        "Permanent Ice Sheet / Impassable Wilderness" => [
            "Inlandsis permanente / naturaleza infranqueable",
            "Calotte glaciaire permanente / nature infranchissable",
            "Permanenter Eisschild / unpassierbare Wildnis",
            "Manto de gelo permanente / natureza intransitável",
            "Kalıcı Buz Tabakası / Geçilemez Vahşi Doğa",
        ],
        "#{}" => ["#{}", "n° {}", "#{}", "#{}", "#{}"],
        "🏔️ Wasteland" => ["🏔️ Yermo", "🏔️ Terre désolée", "🏔️ Ödland", "🏔️ Ermo", "🏔️ Çoraklık"],
        "Impassable / Uninhabited Wilderness" => [
            "Naturaleza infranqueable / deshabitada",
            "Nature infranchissable / inhabitée",
            "Unpassierbare / unbewohnte Wildnis",
            "Natureza intransitável / desabitada",
            "Geçilemez / Issız Vahşi Doğa",
        ],
        "📍 Province #{}" => ["📍 Provincia #{}", "📍 Province n° {}", "📍 Provinz #{}", "📍 Província #{}", "📍 İl #{}"],
        "Population: {} people ({}/km²)" => [
            "Población: {} personas ({}/km²)",
            "Population : {} habitants ({}/km²)",
            "Bevölkerung: {} Menschen ({}/km²)",
            "População: {} pessoas ({}/km²)",
            "Nüfus: {} kişi ({}/km²)",
        ],
        "Society population: {} people" => [
            "Población de la sociedad: {} personas",
            "Population de la société : {} habitants",
            "Bevölkerung der Gesellschaft: {} Menschen",
            "População da sociedade: {} pessoas",
            "Toplum nüfusu: {} kişi",
        ],
        "🌊 Ocean" => ["🌊 Océano", "🌊 Océan", "🌊 Ozean", "🌊 Oceano", "🌊 Okyanus"],
        "{} · {} — {}" => ["{} · {} — {}", "{} · {} — {}", "{} · {} — {}", "{} · {} — {}", "{} · {} — {}"],
        "• {}" => ["• {}", "• {}", "• {}", "• {}", "• {}"],
        "A state" => ["Un estado", "Un État", "Ein Staat", "Um estado", "Bir devlet"],
        "A pre-state society" => [
            "Una sociedad preestatal",
            "Une société pré-étatique",
            "Eine vorstaatliche Gesellschaft",
            "Uma sociedade pré-estatal",
            "Devlet öncesi bir toplum",
        ],
        "{} people · {} provinces · {} km²" => [
            "{} personas · {} provincias · {} km²",
            "{} habitants · {} provinces · {} km²",
            "{} Menschen · {} Provinzen · {} km²",
            "{} pessoas · {} províncias · {} km²",
            "{} kişi · {} il · {} km²",
        ],
        "No neighbour within reach." => [
            "Ningún vecino al alcance.",
            "Aucun voisin à portée.",
            "Kein Nachbar in Reichweite.",
            "Nenhum vizinho ao alcance.",
            "Erişilebilecek komşu yok.",
        ],
        "Neighbours: {}" => ["Vecinos: {}", "Voisins : {}", "Nachbarn: {}", "Vizinhos: {}", "Komşular: {}"],
        // ── The world's vocabulary: climate, terrain, slope ──────────────────
        "Ice cap" => ["Casco de hielo", "Calotte glaciaire", "Eiskappe", "Calota de gelo", "Buz Örtüsü"],
        "Tundra" => ["Tundra", "Tundra", "Tundra", "Tundra", "Tundra"],
        "Cold steppe" => ["Estepa fría", "Steppe froide", "Kaltsteppe", "Estepe fria", "Soğuk bozkır"],
        "Boreal forest" => [
            "Bosque boreal",
            "Forêt boréale",
            "Borealer Wald",
            "Floresta boreal",
            "Boreal orman",
        ],
        "Highland" => ["Tierras altas", "Hautes terres", "Hochland", "Terras altas", "Yayla"],
        "Temperate desert" => [
            "Desierto templado",
            "Désert tempéré",
            "Gemäßigte Wüste",
            "Deserto temperado",
            "Ilıman çöl",
        ],
        "Semi-arid grassland" => [
            "Pradera semiárida",
            "Prairie semi-aride",
            "Halbtrockenes Grasland",
            "Pradaria semiárida",
            "Yarı kurak otlak",
        ],
        "Temperate forest" => [
            "Bosque templado",
            "Forêt tempérée",
            "Gemäßigter Wald",
            "Floresta temperada",
            "Ilıman orman",
        ],
        "Temperate rainforest" => [
            "Selva templada",
            "Forêt pluviale tempérée",
            "Gemäßigter Regenwald",
            "Floresta pluvial temperada",
            "Ilıman yağmur ormanı",
        ],
        "Hot desert" => ["Desierto cálido", "Désert chaud", "Heiße Wüste", "Deserto quente", "Sıcak çöl"],
        "Savanna" => ["Sabana", "Savane", "Savanne", "Savana", "Savan"],
        "Tropical rainforest" => [
            "Selva tropical",
            "Forêt tropicale humide",
            "Tropischer Regenwald",
            "Floresta tropical",
            "Tropik yağmur ormanı",
        ],
        "permanent ice: nothing thaws long enough to grow" => [
            "hielo permanente: nada se deshiela el tiempo suficiente para crecer",
            "glace permanente : rien ne dégèle assez longtemps pour pousser",
            "ewiges Eis: nichts taut lange genug, um zu wachsen",
            "gelo permanente: nada derrete o tempo suficiente para crescer",
            "kalıcı buz: hiçbir şey büyüyecek kadar erimez",
        ],
        "a brief summer over frozen subsoil; moss, lichen and low scrub" => [
            "un breve verano sobre suelo congelado; musgo, líquenes y matorral bajo",
            "un été bref sur un sol gelé ; mousses, lichens et broussailles basses",
            "ein kurzer Sommer über gefrorenem Untergrund; Moos, Flechten und niedriges Gestrüpp",
            "um breve verão sobre solo gelado; musgo, líquenes e mato baixo",
            "donmuş toprak üzerinde kısa bir yaz; yosun, liken ve alçak çalılar",
        ],
        "cold and dry: short grass over hard ground" => [
            "frío y seco: hierba corta sobre suelo duro",
            "froid et sec : herbe courte sur un sol dur",
            "kalt und trocken: kurzes Gras auf hartem Boden",
            "frio e seco: erva curta sobre solo duro",
            "soğuk ve kurak: sert toprak üzerinde kısa çimenler",
        ],
        "long cold winters under a belt of conifers" => [
            "largos inviernos fríos bajo un cinturón de coníferas",
            "de longs hivers froids sous une ceinture de conifères",
            "lange kalte Winter unter einem Gürtel aus Nadelbäumen",
            "longos invernos frios sob um cinturão de coníferas",
            "kozalaklı bir kuşak altında uzun soğuk kışlar",
        ],
        "mountain climate: thin air, hard frost, a short season" => [
            "clima de montaña: aire fino, heladas duras, estación corta",
            "climat de montagne : air raréfié, gelées dures, saison courte",
            "Gebirgsklima: dünne Luft, harter Frost, kurze Saison",
            "clima de montanha: ar rarefeito, geada forte, estação curta",
            "dağ iklimi: ince hava, sert don, kısa mevsim",
        ],
        "temperate but arid: bare ground and thorn scrub" => [
            "templado pero árido: suelo desnudo y matorral espinoso",
            "tempéré mais aride : sol nu et broussailles épineuses",
            "gemäßigt, aber arid: kahler Boden und Dornengestrüpp",
            "temperado mas árido: solo nu e mato espinhoso",
            "ılıman ama kurak: çıplak toprak ve dikenli çalılar",
        ],
        "dry grassland at the edge of the desert" => [
            "pradera seca al borde del desierto",
            "prairie sèche en bordure du désert",
            "trockenes Grasland am Rand der Wüste",
            "pradaria seca à beira do deserto",
            "çölün kenarında kuru otlak",
        ],
        "mild and wet enough for broadleaf woodland" => [
            "suave y húmedo para bosques de hoja ancha",
            "doux et assez humide pour la forêt feuillue",
            "mild und feucht genug für Laubwald",
            "ameno e húmido o suficiente para floresta de folha larga",
            "geniş yapraklı orman için yeterince ılıman ve nemli",
        ],
        "cool, very wet and densely wooded" => [
            "fresco, muy húmedo y densamente arbolado",
            "frais, très humide et densément boisé",
            "kühl, sehr feucht und dicht bewaldet",
            "fresco, muito húmido e densamente arborizado",
            "serin, çok nemli ve sık ormanlı",
        ],
        "hot and almost rainless: sand, gravel and stone" => [
            "caluroso y casi sin lluvia: arena, grava y piedra",
            "chaud et presque sans pluie : sable, gravier et pierre",
            "heiß und fast regenlos: Sand, Kies und Stein",
            "quente e quase sem chuva: areia, cascalho e pedra",
            "sıcak ve neredeyse yağmursuz: kum, çakıl ve taş",
        ],
        "a hot wet season and a long dry one: grass with scattered trees" => [
            "una estación húmeda cálida y otra larga y seca: hierba con árboles dispersos",
            "une saison humide chaude et une longue saison sèche : herbe et arbres épars",
            "eine heiße Regenzeit und eine lange Trockenzeit: Gras mit verstreuten Bäumen",
            "uma estação húmida quente e outra longa e seca: erva com árvores dispersas",
            "sıcak bir yağışlı mevsim ve uzun bir kurak mevsim: seyrek ağaçlı otlak",
        ],
        "hot and wet all year: closed evergreen canopy" => [
            "caluroso y húmedo todo el año: dosel perennifolio cerrado",
            "chaud et humide toute l'année : canopée sempervirente fermée",
            "heiß und feucht das ganze Jahr: geschlossenes immergrünes Kronendach",
            "quente e húmido todo o ano: copa perenifólia fechada",
            "yıl boyu sıcak ve nemli: kapalı yaprak dökmeyen taç örtüsü",
        ],
        "Abyssal plain" => [
            "Llanura abisal",
            "Plaine abyssale",
            "Abyssal-Ebene",
            "Planície abissal",
            "Abisal ova",
        ],
        "Ocean floor" => [
            "Fondo oceánico",
            "Fond océanique",
            "Meeresboden",
            "Fundo oceânico",
            "Okyanus tabanı",
        ],
        "Continental shelf" => [
            "Plataforma continental",
            "Plateau continental",
            "Kontinentalschelf",
            "Plataforma continental",
            "Kıta sahanlığı",
        ],
        "Lake" => ["Lago", "Lac", "See", "Lago", "Göl"],
        "Ice" => ["Hielo", "Glace", "Eis", "Gelo", "Buz"],
        "Coastal lowland" => [
            "Tierras bajas costeras",
            "Plaine côtière",
            "Küstentiefland",
            "Terras baixas costeiras",
            "Kıyı ovası",
        ],
        "Lowland" => ["Tierras bajas", "Plaines", "Tiefland", "Terras baixas", "Ova"],
        "Hill country" => [
            "Región de colinas",
            "Pays de collines",
            "Hügelland",
            "Região de colinas",
            "Tepelik bölge",
        ],
        "Upland" => ["Altiplano", "Hauts plateaux", "Hochland", "Planalto", "Yüksek ova"],
        "Mountain" => ["Montaña", "Montagne", "Bergland", "Montanha", "Dağ"],
        "High mountain" => [
            "Alta montaña",
            "Haute montagne",
            "Hochgebirge",
            "Alta montanha",
            "Yüksek dağ",
        ],
        "below −3 000 m" => [
            "por debajo de −3 000 m",
            "sous −3 000 m",
            "unter −3 000 m",
            "abaixo de −3 000 m",
            "−3 000 m altında",
        ],
        "−3 000 … −200 m" => [
            "−3 000 … −200 m",
            "−3 000 … −200 m",
            "−3 000 … −200 m",
            "−3 000 … −200 m",
            "−3 000 … −200 m",
        ],
        "−200 … 0 m" => ["−200 … 0 m", "−200 … 0 m", "−200 … 0 m", "−200 … 0 m", "−200 … 0 m"],
        "lake surface" => [
            "superficie del lago",
            "surface du lac",
            "Seespiegel",
            "superfície do lago",
            "göl yüzeyi",
        ],
        "frozen ground or sea" => [
            "suelo o mar helado",
            "sol ou mer gelés",
            "gefrorener Boden oder gefrorenes Meer",
            "solo ou mar gelado",
            "donmuş toprak veya deniz",
        ],
        "0 … 150 m" => ["0 … 150 m", "0 … 150 m", "0 … 150 m", "0 … 150 m", "0 … 150 m"],
        "150 … {} m" => ["150 … {} m", "150 … {} m", "150 … {} m", "150 … {} m", "150 … {} m"],
        "{} … {} m" => ["{} … {} m", "{} … {} m", "{} … {} m", "{} … {} m", "{} … {} m"],
        "{} m +" => ["{} m y más", "{} m et plus", "{} m +", "{} m +", "{} m ve üzeri"],
        "gentle" => ["suave", "douce", "sanft", "suave", "hafif"],
        "rolling" => ["ondulada", "vallonnée", "wellig", "ondulada", "dalgalı"],
        "steep" => ["empinada", "raide", "steil", "íngreme", "dik"],
        "cliff-like" => [
            "como un acantilado",
            "abrupte",
            "felsig",
            "como uma falésia",
            "uçurum gibi",
        ],
        // ── The configuration panel ──────────────────────────────────────────
        "Every constant of the engine, in the order the acts apply it. A value applies to the acts not yet grown — press ↺ on an act to grow it again." => [
            "Todas las constantes del motor, en el orden en que las aplican los actos. Un valor afecta a los actos aún no generados: pulsa ↺ en un acto para generarlo de nuevo.",
            "Toutes les constantes du moteur, dans l'ordre où les actes les appliquent. Une valeur s'applique aux actes pas encore générés — appuyez sur ↺ sur un acte pour le régénérer.",
            "Jede Konstante der Engine, in der Reihenfolge, in der die Akte sie anwenden. Ein Wert gilt für noch nicht gewachsene Akte — drücken Sie ↺ bei einem Akt, um ihn erneut zu erzeugen.",
            "Todas as constantes do motor, na ordem em que os atos as aplicam. Um valor aplica-se aos atos ainda não gerados — prima ↺ num ato para o gerar de novo.",
            "Motorun tüm sabitleri, perdelerin uyguladığı sırayla. Bir değer henüz oluşmamış perdeler için geçerlidir — bir perdeyi yeniden oluşturmak için ↺ düğmesine basın.",
        ],
        "↺ Reset all" => [
            "↺ Restablecer todo",
            "↺ Tout réinitialiser",
            "↺ Alles zurücksetzen",
            "↺ Repor tudo",
            "↺ Tümünü sıfırla",
        ],
        "Every constant back to the engine's default" => [
            "Todas las constantes vuelven al valor por defecto del motor",
            "Toutes les constantes reviennent à la valeur par défaut du moteur",
            "Alle Konstanten zurück auf den Standard der Engine",
            "Todas as constantes voltam ao valor padrão do motor",
            "Tüm sabitler motorun varsayılanına döner",
        ],
        "Everything is already at the engine's defaults" => [
            "Todo está ya en los valores por defecto del motor",
            "Tout est déjà aux valeurs par défaut du moteur",
            "Alles steht bereits auf den Standardwerten der Engine",
            "Está tudo já nos valores padrão do motor",
            "Her şey zaten motorun varsayılanında",
        ],
        "· an act is running" => [
            "· hay un acto en curso",
            "· un acte est en cours",
            "· ein Akt läuft",
            "· um ato está a correr",
            "· bir perde çalışıyor",
        ],
        "⚙ Advanced configuration  ·  edited" => [
            "⚙ Configuración avanzada  ·  modificada",
            "⚙ Configuration avancée  ·  modifiée",
            "⚙ Erweiterte Konfiguration  ·  geändert",
            "⚙ Configuração avançada  ·  alterada",
            "⚙ Gelişmiş yapılandırma  ·  değiştirildi",
        ],
        "⚙ Advanced configuration" => [
            "⚙ Configuración avanzada",
            "⚙ Configuration avancée",
            "⚙ Erweiterte Konfiguration",
            "⚙ Configuração avançada",
            "⚙ Gelişmiş yapılandırma",
        ],
        "🪐 Planetary & astronomical" => [
            "🪐 Planetario y astronómico",
            "🪐 Planétaire et astronomique",
            "🪐 Planetar und astronomisch",
            "🪐 Planetário e astronómico",
            "🪐 Gezegen ve astronomi",
        ],
        "🧊 Polar glaciation" => [
            "🧊 Glaciación polar",
            "🧊 Glaciation polaire",
            "🧊 Polare Vereisung",
            "🧊 Glaciação polar",
            "🧊 Kutup buzullaşması",
        ],
        "🗺 Continental layout & coastlines" => [
            "🗺 Disposición continental y costas",
            "🗺 Disposition des continents et littoraux",
            "🗺 Kontinentallayout und Küsten",
            "🗺 Disposição continental e costas",
            "🗺 Kıta düzeni ve kıyılar",
        ],
        "⛰ Orogeny & tectonics" => [
            "⛰ Orogénesis y tectónica",
            "⛰ Orogenèse et tectonique",
            "⛰ Orogenese und Tektonik",
            "⛰ Orogénese e tectónica",
            "⛰ Dağ oluşumu ve tektonik",
        ],
        "💧 Weathering & erosion" => [
            "💧 Meteorización y erosión",
            "💧 Altération et érosion",
            "💧 Verwitterung und Erosion",
            "💧 Meteorização e erosão",
            "💧 Ayrışma ve erozyon",
        ],
        "🌧 Climate & atmospheric moisture" => [
            "🌧 Clima y humedad atmosférica",
            "🌧 Climat et humidité atmosphérique",
            "🌧 Klima und Luftfeuchte",
            "🌧 Clima e humidade atmosférica",
            "🌧 İklim ve atmosferik nem",
        ],
        "🏞 Hydrology & rivers" => [
            "🏞 Hidrología y ríos",
            "🏞 Hydrologie et rivières",
            "🏞 Hydrologie und Flüsse",
            "🏞 Hidrologia e rios",
            "🏞 Hidroloji ve nehirler",
        ],
        "🧭 Geography, provinces & wastelands" => [
            "🧭 Geografía, provincias y yermos",
            "🧭 Géographie, provinces et terres désolées",
            "🧭 Geografie, Provinzen und Ödländer",
            "🧭 Geografia, províncias e ermos",
            "🧭 Coğrafya, iller ve çorak topraklar",
        ],
        "👑 Geopolitical hierarchy" => [
            "👑 Jerarquía geopolítica",
            "👑 Hiérarchie géopolitique",
            "👑 Geopolitische Hierarchie",
            "👑 Hierarquia geopolítica",
            "👑 Jeopolitik hiyerarşi",
        ],
        "Act 2 — the planet's spin and its sea-level temperatures" => [
            "Acto 2: la rotación del planeta y sus temperaturas al nivel del mar",
            "Acte 2 — la rotation de la planète et ses températures au niveau de la mer",
            "Akt 2 — der Spin des Planeten und seine Temperaturen auf Meereshöhe",
            "Ato 2 — a rotação do planeta e as suas temperaturas ao nível do mar",
            "2. perde — gezegenin dönüşü ve deniz seviyesindeki sıcaklıkları",
        ],
        "Acts 1 & 3 — where the permanent ice lies, in degrees of latitude" => [
            "Actos 1 y 3: dónde está el hielo permanente, en grados de latitud",
            "Actes 1 et 3 — où se trouve la glace permanente, en degrés de latitude",
            "Akte 1 und 3 — wo das ständige Eis liegt, in Breitengraden",
            "Atos 1 e 3 — onde está o gelo permanente, em graus de latitude",
            "1. ve 3. perde — kalıcı buzun bulunduğu enlemler",
        ],
        "Act 1 — the continental field and its water line" => [
            "Acto 1: el campo continental y su línea de agua",
            "Acte 1 — le champ continental et sa ligne d'eau",
            "Akt 1 — das Kontinentalfeld und seine Wasserlinie",
            "Ato 1 — o campo continental e a sua linha de água",
            "1. perde — kıta alanı ve su çizgisi",
        ],
        "Act 1 — the faults that raise the ranges" => [
            "Acto 1: las fallas que levantan las cordilleras",
            "Acte 1 — les failles qui soulèvent les chaînes",
            "Akt 1 — die Verwerfungen, die Gebirge aufwerfen",
            "Ato 1 — as falhas que levantam as cordilheiras",
            "1. perde — sıradağları yükselten faylar",
        ],
        "Act 1 — how the land is worn down after it rises" => [
            "Acto 1: cómo se desgasta la tierra después de elevarse",
            "Acte 1 — comment la terre s'use après son soulèvement",
            "Akt 1 — wie das Land nach seinem Aufstieg abgetragen wird",
            "Ato 1 — como a terra se desgasta depois de se elevar",
            "1. perde — toprak yükseldikten sonra nasıl aşınır",
        ],
        "Act 2 — the moist atmosphere" => [
            "Acto 2: la atmósfera húmeda",
            "Acte 2 — l'atmosphère humide",
            "Akt 2 — die feuchte Atmosphäre",
            "Ato 2 — a atmosfera húmida",
            "2. perde — nemli atmosfer",
        ],
        "Act 3 — what reaches the sea" => [
            "Acto 3: lo que llega al mar",
            "Acte 3 — ce qui atteint la mer",
            "Akt 3 — was das Meer erreicht",
            "Ato 3 — o que chega ao mar",
            "3. perde — denize ulaşan",
        ],
        "Acts 4 & 6 — the land and the ocean cut into provinces" => [
            "Actos 4 y 6: la tierra y el océano divididos en provincias",
            "Actes 4 et 6 — la terre et l'océan découpés en provinces",
            "Akte 4 und 6 — Land und Ozean in Provinzen gegliedert",
            "Atos 4 e 6 — a terra e o oceano divididos em províncias",
            "4. ve 6. perde — kara ve okyanusun illere bölünmesi",
        ],
        "Act 5 — how many continents the world is allowed" => [
            "Acto 5: cuántos continentes se permite al mundo",
            "Acte 5 — combien de continents le monde a le droit d'avoir",
            "Akt 5 — wie viele Kontinente die Welt haben darf",
            "Ato 5 — quantos continentes o mundo pode ter",
            "5. perde — dünyaya kaç kıta izin verildiği",
        ],
        "{}  ·  edited" => [
            "{}  ·  modificada",
            "{}  ·  modifiée",
            "{}  ·  geändert",
            "{}  ·  alterada",
            "{}  ·  değiştirildi",
        ],
        "↺ Reset these to the engine's defaults" => [
            "↺ Restablecer estos valores por defecto",
            "↺ Rétablir les valeurs par défaut",
            "↺ Diese auf den Standard zurücksetzen",
            "↺ Repor estes valores padrão",
            "↺ Bunları varsayılana sıfırla",
        ],
        "Planetary rotation mode" => [
            "Modo de rotación planetaria",
            "Mode de rotation planétaire",
            "Planetarer Rotationsmodus",
            "Modo de rotação planetária",
            "Gezegen dönüş modu",
        ],
        "Controls the planetary dynamic: a conventional rotating globe with Hadley/Ferrel circulation, or an eyeball world tidally locked to its sun, with a permanent day face and a permanent night face." => [
            "Controla la dinámica planetaria: un globo en rotación convencional con circulación de Hadley/Ferrel, o un mundo globo ocular anclado marealmente a su sol, con una cara diurna y otra nocturna permanentes.",
            "Contrôle la dynamique planétaire : un globe en rotation classique avec circulation de Hadley/Ferrel, ou un monde « œil » verrouillé par effet de marée à son soleil, avec une face jour et une face nuit permanentes.",
            "Steuert die planetare Dynamik: ein konventionell rotierender Globus mit Hadley/Ferrel-Zirkulation oder eine an ihre Sonne gebundene Augapfelwelt mit dauerhafter Tag- und Nachtseite.",
            "Controla a dinâmica planetária: um globo em rotação convencional com circulação de Hadley/Ferrel, ou um mundo «olho» bloqueado por maré ao seu sol, com uma face diurna e outra noturna permanentes.",
            "Gezegen dinamiklerini denetler: Hadley/Ferrel dolaşımı olan normal dönen bir küre ya da güneşine kilitlenmiş, kalıcı gündüz ve gece yüzleri olan bir göz küresi dünya.",
        ],
        "Rotating" => ["En rotación", "En rotation", "Rotierend", "Em rotação", "Dönen"],
        "Tidally locked (eyeball)" => [
            "Anclado marealmente (globo ocular)",
            "Verrouillé par marée (œil)",
            "Gezeitenfest (Augapfel)",
            "Bloqueado por maré (olho)",
            "Gelgit kilitli (göz küresi)",
        ],
        "Rotating (day & night)" => [
            "En rotación (día y noche)",
            "En rotation (jour et nuit)",
            "Rotierend (Tag und Nacht)",
            "Em rotação (dia e noite)",
            "Dönen (gündüz ve gece)",
        ],
        "Back to the engine's default ({})" => [
            "Volver al valor por defecto del motor ({})",
            "Revenir à la valeur par défaut du moteur ({})",
            "Zurück zum Standard der Engine ({})",
            "Voltar ao valor padrão do motor ({})",
            "Motorun varsayılanına dön ({})",
        ],
        "Already the engine's default ({})" => [
            "Ya es el valor por defecto del motor ({})",
            "Déjà la valeur par défaut du moteur ({})",
            "Bereits der Standard der Engine ({})",
            "Já é o valor padrão do motor ({})",
            "Zaten motorun varsayılanı ({})",
        ],
        "{}\n\nEngine default: {}" => [
            "{}\n\nValor por defecto del motor: {}",
            "{}\n\nValeur par défaut du moteur : {}",
            "{}\n\nEngine-Standard: {}",
            "{}\n\nValor padrão do motor: {}",
            "{}\n\nMotor varsayılanı: {}",
        ],
        "⚠ {}" => ["⚠ {}", "⚠ {}", "⚠ {}", "⚠ {}", "⚠ {}"],
        "Equatorial temperature" => [
            "Temperatura ecuatorial",
            "Température équatoriale",
            "Äquatortemperatur",
            "Temperatura equatorial",
            "Ekvator sıcaklığı",
        ],
        "Polar temperature" => [
            "Temperatura polar",
            "Température polaire",
            "Polartemperatur",
            "Temperatura polar",
            "Kutup sıcaklığı",
        ],
        "Land ice cap edge" => [
            "Borde del casquete glaciar",
            "Limite de la calotte glaciaire",
            "Rand der Landeiskappe",
            "Limite da calota de gelo",
            "Kara buz örtüsü sınırı",
        ],
        "Permanent sea ice edge" => [
            "Borde del hielo marino permanente",
            "Limite de la banquise permanente",
            "Rand des ständigen Meereises",
            "Limite do gelo marinho permanente",
            "Kalıcı deniz buzu sınırı",
        ],
        "Sea level / ocean balance" => [
            "Nivel del mar / equilibrio oceánico",
            "Niveau de la mer / équilibre océanique",
            "Meeresspiegel / Ozeangleichgewicht",
            "Nível do mar / equilíbrio oceânico",
            "Deniz seviyesi / okyanus dengesi",
        ],
        "Landmass scale" => [
            "Escala de las masas de tierra",
            "Échelle des masses continentales",
            "Landmassen-Skala",
            "Escala das massas de terra",
            "Kara kütlesi ölçeği",
        ],
        "Coastline roughness" => [
            "Rugosidad de la costa",
            "Rugosité des côtes",
            "Küstenrauheit",
            "Rugosidade da costa",
            "Kıyı pürüzlülüğü",
        ],
        "Coastline distortion / fjords" => [
            "Distorsión de la costa / fiordos",
            "Déformation des côtes / fjords",
            "Küstenverzerrung / Fjorde",
            "Distorção da costa / fiordes",
            "Kıyı bozulması / fiyortlar",
        ],
        "Mountain uplift height" => [
            "Altura del levantamiento montañoso",
            "Hauteur du soulèvement montagneux",
            "Höhe der Gebirgshebung",
            "Altura do levantamento montanhoso",
            "Dağ yükselme yüksekliği",
        ],
        "Mountain chain spacing" => [
            "Separación de las cordilleras",
            "Espacement des chaînes",
            "Abstand der Gebirgsketten",
            "Espaçamento das cordilheiras",
            "Sıradağ aralığı",
        ],
        "Mountain belt width" => [
            "Ancho de la franja montañosa",
            "Largeur de la ceinture montagneuse",
            "Breite des Gebirgsgürtels",
            "Largura da faixa montanhosa",
            "Dağ kuşağı genişliği",
        ],
        "Hydraulic erosion droplets" => [
            "Gotas de erosión hídrica",
            "Gouttes d'érosion hydraulique",
            "Tropfen der hydraulischen Erosion",
            "Gotas de erosão hídrica",
            "Hidrolik erozyon damlaları",
        ],
        "Thermal weathering passes" => [
            "Pasadas de meteorización térmica",
            "Passes d'altération thermique",
            "Durchläufe thermischer Verwitterung",
            "Passagens de meteorização térmica",
            "Termal ayrışma geçişleri",
        ],
        "Global baseline rainfall" => [
            "Lluvia base global",
            "Précipitations de base globales",
            "Globaler Grundniederschlag",
            "Precipitação base global",
            "Küresel temel yağış",
        ],
        "Subtropical desert severity" => [
            "Severidad del desierto subtropical",
            "Intensité des déserts subtropicaux",
            "Stärke der subtropischen Wüsten",
            "Severidade do deserto subtropical",
            "Subtropik çöl şiddeti",
        ],
        "Interior aridity floor" => [
            "Mínimo de aridez interior",
            "Seuil d'aridité intérieure",
            "Untergrenze der Binnenaridität",
            "Mínimo de aridez interior",
            "İç kuraklık tabanı",
        ],
        "Rain shadow intensity" => [
            "Intensidad de la sombra de lluvia",
            "Intensité de l'ombre pluviométrique",
            "Stärke des Regenschattens",
            "Intensidade da sombra de chuva",
            "Yağmur gölgesi şiddeti",
        ],
        "Surface runoff fraction" => [
            "Fracción de escorrentía superficial",
            "Fraction de ruissellement",
            "Anteil des Oberflächenabflusses",
            "Fração de escoamento superficial",
            "Yüzey akış oranı",
        ],
        "River network cutoff" => [
            "Umbral de la red fluvial",
            "Seuil du réseau fluvial",
            "Schwelle des Flussnetzes",
            "Limite da rede fluvial",
            "Nehir ağı eşiği",
        ],
        "Land provinces" => [
            "Provincias terrestres",
            "Provinces terrestres",
            "Landprovinzen",
            "Províncias terrestres",
            "Kara illeri",
        ],
        "Marine provinces" => [
            "Provincias marinas",
            "Provinces marines",
            "Marine Provinzen",
            "Províncias marinhas",
            "Deniz illeri",
        ],
        "Impassable wilderness cap" => [
            "Límite de naturaleza infranqueable",
            "Plafond de nature infranchissable",
            "Obergrenze unpassierbarer Wildnis",
            "Limite de natureza intransitável",
            "Geçilemez vahşi doğa üst sınırı",
        ],
        "Wasteland hostility cutoff" => [
            "Umbral de hostilidad del yermo",
            "Seuil d'hostilité des terres désolées",
            "Schwelle der Ödlandfeindlichkeit",
            "Limite de hostilidade do ermo",
            "Çorak toprak düşmanlık eşiği",
        ],
        "Max continents" => [
            "Continentes máximos",
            "Continents maximum",
            "Maximale Kontinente",
            "Continentes máximos",
            "En fazla kıta",
        ],
        "Sea-level surface temperature at the equator. Higher values expand the tropical belt; lower values begin a global cooling." => [
            "Temperatura superficial al nivel del mar en el ecuador. Valores más altos amplían el cinturón tropical; los más bajos inician un enfriamiento global.",
            "Température de surface au niveau de la mer à l'équateur. Des valeurs plus élevées élargissent la ceinture tropicale ; les plus basses amorcent un refroidissement global.",
            "Oberflächentemperatur auf Meereshöhe am Äquator. Höhere Werte weiten den Tropengürtel aus, niedrigere beginnen eine globale Abkühlung.",
            "Temperatura à superfície ao nível do mar no equador. Valores mais altos ampliam o cinturão tropical; os mais baixos iniciam um arrefecimento global.",
            "Ekvator'da deniz seviyesindeki yüzey sıcaklığı. Yüksek değerler tropik kuşağı genişletir; düşük değerler küresel bir soğuma başlatır.",
        ],
        "Sea-level surface temperature at the poles. Above 0 °C the world is a greenhouse without permanent ice." => [
            "Temperatura superficial al nivel del mar en los polos. Por encima de 0 °C el mundo es un invernadero sin hielo permanente.",
            "Température de surface au niveau de la mer aux pôles. Au-dessus de 0 °C, le monde est un effet de serre sans glace permanente.",
            "Oberflächentemperatur auf Meereshöhe an den Polen. Über 0 °C ist die Welt ein Treibhaus ohne dauerhaftes Eis.",
            "Temperatura à superfície ao nível do mar nos polos. Acima de 0 °C o mundo é uma estufa sem gelo permanente.",
            "Kutuplarda deniz seviyesindeki yüzey sıcaklığı. 0 °C'nin üzerinde dünya, kalıcı buzu olmayan bir sera olur.",
        ],
        "The latitude where the thick continental ice caps begin to form. Lower it for an ice age; raise it to keep the glaciers at the poles. The sheet reaches solid ice 0.18 rad (≈ 10°) further poleward." => [
            "La latitud donde empiezan a formarse los gruesos casquetes glaciares continentales. Bájala para una edad de hielo; súbela para mantener los glaciares en los polos. La capa alcanza hielo sólido 0,18 rad (≈ 10°) más hacia el polo.",
            "La latitude où les épaisses calottes glaciaires continentales commencent à se former. Abaissez-la pour un âge glaciaire ; élevez-la pour garder les glaciers aux pôles. La calotte atteint la glace solide 0,18 rad (≈ 10°) plus loin vers le pôle.",
            "Die Breite, in der die dicken kontinentalen Eiskappen zu entstehen beginnen. Niedriger für eine Eiszeit, höher, um die Gletscher an den Polen zu halten. Der Schild erreicht festes Eis 0,18 rad (≈ 10°) weiter polwärts.",
            "A latitude onde as espessas calotas polares continentais começam a formar-se. Baixe-a para uma era glaciar; suba-a para manter os glaciares nos polos. O manto atinge gelo sólido 0,18 rad (≈ 10°) mais para o polo.",
            "Kalın kıta buz örtülerinin oluşmaya başladığı enlem. Buzul çağı için düşürün; buzulları kutuplarda tutmak için yükseltin. Tabaka, kutba doğru 0,18 rad (≈ 10°) daha ileride katı buza ulaşır.",
        ],
        "The latitude where permanent pack ice forms over the ocean. The sea freezes slightly equatorward of the land ice, so this edge is held at or below the ice cap's." => [
            "La latitud donde se forma hielo marino permanente sobre el océano. El mar se congela ligeramente hacia el ecuador respecto al hielo terrestre, así que este borde se mantiene igual o por debajo del casquete.",
            "La latitude où la banquise permanente se forme sur l'océan. La mer gèle légèrement plus près de l'équateur que la glace terrestre ; cette limite est donc maintenue au niveau de la calotte ou en dessous.",
            "Die Breite, in der sich ständiges Packeis auf dem Ozean bildet. Das Meer gefriert etwas äquatorwärts des Landeises, daher liegt dieser Rand auf oder unter dem der Eiskappe.",
            "A latitude onde se forma gelo marinho permanente sobre o oceano. O mar congela ligeiramente mais perto do equador do que o gelo terrestre, pelo que este limite fica igual ou abaixo do da calota.",
            "Okyanus üzerinde kalıcı deniz buzunun oluştuğu enlem. Deniz, kara buzuna göre ekvatora biraz daha yakın donar; bu sınır buz örtüsünün sınırında ya da altında tutulur.",
        ],
        "The sea-ice edge is poleward of the ice cap: the sea freezes first, so the belt is pulled back to the cap when the act runs." => [
            "El borde del hielo marino está más al polo que el casquete: el mar se congela antes, así que la franja se ajusta al casquete cuando se ejecuta el acto.",
            "La limite de la banquise est plus polaire que la calotte : la mer gèle d'abord, la ceinture est donc ramenée à la calotte à l'exécution de l'acte.",
            "Der Meereisrand liegt polwärts der Eiskappe: Das Meer gefriert zuerst, daher wird der Gürtel beim Lauf des Akts auf die Kappe zurückgezogen.",
            "O limite do gelo marinho está mais para o polo do que a calota: o mar congela primeiro, pelo que a faixa é recuada até à calota quando o ato corre.",
            "Deniz buzu sınırı buz örtüsünün kutup tarafında: deniz önce donduğu için perde çalıştığında kuşak buz örtüsüne çekilir.",
        ],
        "Shifts the continental datum against the water line: 0.02 drains the ocean basins to reveal land bridges and inland seas, 0.11 is Earth, 0.22 drowns the continents into archipelagos." => [
            "Desplaza el datum continental respecto a la línea de agua: 0,02 drena las cuencas oceánicas y revela puentes de tierra y mares interiores, 0,11 es la Tierra, 0,22 ahoga los continentes en archipiélagos.",
            "Décale le datum continental par rapport à la ligne d'eau : 0,02 draine les bassins océaniques et révèle des ponts terrestres et des mers intérieures, 0,11 c'est la Terre, 0,22 noie les continents en archipels.",
            "Verschiebt das kontinentale Datum gegen die Wasserlinie: 0,02 entleert die Ozeanbecken und legt Landbrücken und Binnenmeere frei, 0,11 ist die Erde, 0,22 ertränkt die Kontinente in Archipele.",
            "Desloca o datum continental em relação à linha de água: 0,02 drena as bacias oceânicas e revela pontes de terra e mares interiores, 0,11 é a Terra, 0,22 afoga os continentes em arquipélagos.",
            "Kıta datumunu su çizgisine göre kaydırır: 0,02 okyanus havzalarını boşaltıp kara köprülerini ve iç denizleri açığa çıkarır, 0,11 Dünya'dır, 0,22 kıtaları takımadalara boğar.",
        ],
        "Frequency of the continental field: 0.40 aggregates the crust into one or two supercontinents, 1.05 is Earth's continents, 4.50 shatters the land into hundreds of islands." => [
            "Frecuencia del campo continental: 0,40 agrupa la corteza en uno o dos supercontinentes, 1,05 son los continentes terrestres, 4,50 fragmenta la tierra en cientos de islas.",
            "Fréquence du champ continental : 0,40 regroupe la croûte en un ou deux supercontinents, 1,05 correspond aux continents terrestres, 4,50 fragmente la terre en centaines d'îles.",
            "Frequenz des Kontinentalfelds: 0,40 bündelt die Kruste zu einem oder zwei Superkontinenten, 1,05 sind die Kontinente der Erde, 4,50 zersplittert das Land in Hunderte Inseln.",
            "Frequência do campo continental: 0,40 agrupa a crusta em um ou dois supercontinentes, 1,05 são os continentes da Terra, 4,50 fragmenta a terra em centenas de ilhas.",
            "Kıta alanının frekansı: 0,40 kabuğu bir ya da iki süper kıtada toplar, 1,05 Dünya'nın kıtalarıdır, 4,50 karayı yüzlerce adaya parçalar.",
        ],
        "Fractal persistence across the continental field's octaves. Low values give smooth, gentle shorelines; high values craggy, rugged coasts with abundant micro-islands." => [
            "Persistencia fractal entre las octavas del campo continental. Valores bajos dan costas suaves y amables; valores altos, costas escarpadas y abruptas con abundantes microislas.",
            "Persistance fractale entre les octaves du champ continental. Des valeurs basses donnent des rivages lisses et doux ; des valeurs hautes, des côtes escarpées et accidentées riches en micro-îles.",
            "Fraktale Persistenz über die Oktaven des Kontinentalfelds. Niedrige Werte ergeben glatte, sanfte Küsten; hohe Werte zerklüftete, raue Küsten mit vielen Mikroinseln.",
            "Persistência fractal entre as oitavas do campo continental. Valores baixos dão costas suaves e gentis; valores altos, costas escarpadas e acidentadas com muitas microilhas.",
            "Kıta alanının oktavları arasında fraktal kalıcılık. Düşük değerler pürüzsüz, yumuşak kıyılar; yüksek değerler bol mikro adalı, sarp ve engebeli kıyılar verir.",
        ],
        "Strength of the coordinate warp on the crust. At 0.00 the landmasses are regular and rounded; at high values coastlines fold into deep fjords, sweeping bays and chaotic peninsulas." => [
            "Fuerza de la deformación de coordenadas sobre la corteza. Con 0,00 las masas de tierra son regulares y redondeadas; con valores altos las costas se pliegan en fiordos profundos, bahías amplias y penínsulas caóticas.",
            "Force de la déformation des coordonnées sur la croûte. À 0,00, les masses continentales sont régulières et arrondies ; à des valeurs élevées, les côtes se plissent en fjords profonds, baies amples et presqu'îles chaotiques.",
            "Stärke der Koordinatenkrümmung auf der Kruste. Bei 0,00 sind die Landmassen regelmäßig und rund; bei hohen Werten falten sich die Küsten zu tiefen Fjorden, weiten Buchten und chaotischen Halbinseln.",
            "Força da distorção de coordenadas na crusta. A 0,00 as massas de terra são regulares e arredondadas; com valores altos as costas dobram-se em fiordes profundos, baías amplas e penínsulas caóticas.",
            "Kabuk üzerindeki koordinat bükülmesinin gücü. 0,00'da kara kütleleri düzenli ve yuvarlaktır; yüksek değerlerde kıyılar derin fiyortlara, geniş koylara ve kaotik yarımadalara katlanır.",
        ],
        "Maximum vertical uplift along a fault line. Low values leave weathered, low-relief shield plains; high values thrust up towering, impassable Alpine ridges." => [
            "Levantamiento vertical máximo a lo largo de una falla. Valores bajos dejan llanuras escudadas y desgastadas; valores altos alzan crestas alpinas imponentes e intransitables.",
            "Soulèvement vertical maximal le long d'une faille. Des valeurs basses laissent des plaines de bouclier altérées et peu accidentées ; des valeurs hautes dressent de hautes crêtes alpines infranchissables.",
            "Maximale vertikale Hebung entlang einer Verwerfung. Niedrige Werte hinterlassen verwitterte Schildplainen mit geringem Relief; hohe Werte türmen gewaltige, unpassierbare Alpenkämme auf.",
            "Levantamento vertical máximo ao longo de uma falha. Valores baixos deixam planícies de escudo desgastadas; valores altos erguem cristas alpinas imponentes e intransitáveis.",
            "Bir fay hattı boyunca en yüksek dikey yükselme. Düşük değerler aşınmış, az engebeli kalkan ovaları bırakır; yüksek değerler görkemli, geçilemez Alp sırtları yükseltir.",
        ],
        "Density of the tectonic fault lines across the globe. Low values yield a few isolated mountain belts; high values a dense, criss-crossed web of ranges." => [
            "Densidad de las fallas tectónicas en el globo. Valores bajos producen unas pocas cordilleras aisladas; valores altos, una red densa y entrecruzada de sierras.",
            "Densité des failles tectoniques sur le globe. Des valeurs basses donnent quelques chaînes isolées ; des valeurs hautes un réseau dense et entrecroisé de montagnes.",
            "Dichte der tektonischen Verwerfungen auf dem Globus. Niedrige Werte ergeben wenige isolierte Gebirgsgürtel; hohe Werte ein dichtes, gekreuztes Netz von Ketten.",
            "Densidade das falhas tectónicas no globo. Valores baixos dão poucas cordilheiras isoladas; valores altos, uma rede densa e entrecruzada de serras.",
            "Dünya genelindeki tektonik fay hatlarının yoğunluğu. Düşük değerler birkaç yalıtılmış dağ kuşağı; yüksek değerler yoğun, çapraz bir sıradağ ağı verir.",
        ],
        "Thickness of the orogenic collision zone. Small values draw sharp, narrow mountain spines; large values build broad, sprawling highland plateaus." => [
            "Grosor de la zona de colisión orogénica. Valores pequeños dibujan crestas afiladas y estrechas; valores grandes construyen amplias mesetas elevadas y extensas.",
            "Épaisseur de la zone de collision orogénique. De petites valeurs dessinent des crêtes acérées et étroites ; de grandes valeurs bâtissent de vastes plateaux d'altitude.",
            "Dicke der orogenen Kollisionszone. Kleine Werte zeichnen scharfe, schmale Gebirgskämme; große Werte bauen breite, ausgedehnte Hochplateaus.",
            "Espessura da zona de colisão orogénica. Valores pequenos desenham cristas afiadas e estreitas; valores grandes constroem amplos planaltos elevados.",
            "Orojenik çarpışma bölgesinin kalınlığı. Küçük değerler keskin, dar dağ sırtları çizer; büyük değerler geniş, yayılmış yüksek platolar oluşturur.",
        ],
        "Number of simulated raindrops running downhill. Low values leave freshly uplifted, jagged crags; high values carve deep canyons, sediment basins and mature dendritic valleys. This is the dominant cost of act 1." => [
            "Número de gotas de lluvia simuladas que corren cuesta abajo. Valores bajos dejan crestas escarpadas recién elevadas; valores altos excavan cañones profundos, cuencas de sedimento y valles dendríticos maduros. Es el coste dominante del acto 1.",
            "Nombre de gouttes de pluie simulées dévalant les pentes. Des valeurs basses laissent des crêtes dentelées fraîchement soulevées ; des valeurs hautes creusent de profonds canyons, des bassins sédimentaires et des vallées dendritiques matures. C'est le coût dominant de l'acte 1.",
            "Anzahl der simulierten Regentropfen, die bergab laufen. Niedrige Werte hinterlassen frisch gehobene, zackige Kämme; hohe Werte graben tiefe Canyons, Sedimentbecken und reife dendritische Täler. Dies ist der dominante Aufwand von Akt 1.",
            "Número de gotas de chuva simuladas a descer a encosta. Valores baixos deixam cristas dentadas recém-elevadas; valores altos escavam canhões profundos, bacias de sedimento e vales dendríticos maduros. É o custo dominante do ato 1.",
            "Yokuş aşağı akan benzetilmiş yağmur damlalarının sayısı. Düşük değerler yeni yükselmiş, sivri kayalıklar bırakır; yüksek değerler derin kanyonlar, tortul havzalar ve olgun dallanan vadiler oyar. 1. perdenin baskın maliyetidir.",
        ],
        "Iterations of talus slumping downhill. At 0 the cliffs stay near-vertical and jagged; higher values soften them into stable, natural scree slopes." => [
            "Iteraciones de deslizamiento de derrubios cuesta abajo. Con 0 los acantilados quedan casi verticales y escarpados; valores mayores los suavizan en laderas de grava estables y naturales.",
            "Itérations d'éboulement des talus vers le bas. À 0, les falaises restent quasi verticales et déchiquetées ; des valeurs plus hautes les adoucissent en pentes d'éboulis stables et naturelles.",
            "Durchläufe des Hangrutschens von Schutt. Bei 0 bleiben die Klippen fast senkrecht und zackig; höhere Werte glätten sie zu stabilen, natürlichen Schutthängen.",
            "Iterações de deslizamento de detritos encosta abaixo. A 0 as falésias ficam quase verticais e dentadas; valores mais altos suavizam-nas em vertentes de pedra solta estáveis e naturais.",
            "Yamaç molozunun aşağı kayma yinelemesi. 0'da uçurumlar neredeyse dikey ve sivri kalır; yüksek değerler onları kararlı, doğal moloz yamaçlarına yumuşatır.",
        ],
        "Baseline precipitation before latitude belts, rain shadows and distance from the sea are accounted for. Sets how lush or dry the planet is." => [
            "Precipitación base antes de considerar cinturones de latitud, sombras de lluvia y distancia al mar. Determina cuán frondoso o seco es el planeta.",
            "Précipitations de base avant prise en compte des ceintures de latitude, des ombres pluviométriques et de la distance à la mer. Détermine la luxuriance ou la sécheresse de la planète.",
            "Grundniederschlag, bevor Breitengürtel, Regenschatten und Meeresferne berücksichtigt werden. Legt fest, wie üppig oder trocken der Planet ist.",
            "Precipitação base antes de considerar cinturões de latitude, sombras de chuva e distância ao mar. Determina quão exuberante ou seco é o planeta.",
            "Enlem kuşakları, yağmur gölgeleri ve denize uzaklık hesaba katılmadan önceki temel yağış. Gezegenin ne kadar yemyeşil ya da kurak olduğunu belirler.",
        ],
        "How deeply the descending limb of the Hadley cell dries the subtropics (around ±26°). At 0.00 subtropical deserts disappear; high values girdle the globe in Sahara-like belts." => [
            "Cuánto seca los subtrópicos la rama descendente de la célula de Hadley (alrededor de ±26°). Con 0,00 los desiertos subtropicales desaparecen; valores altos rodean el globo con franjas tipo Sahara.",
            "À quel point la branche descendante de la cellule de Hadley assèche les subtropiques (autour de ±26°). À 0,00, les déserts subtropicaux disparaissent ; des valeurs hautes ceinturent le globe de bandes sahariennes.",
            "Wie stark der absteigende Ast der Hadley-Zelle die Subtropen austrocknet (um ±26°). Bei 0,00 verschwinden die subtropischen Wüsten; hohe Werte umgürten den Globus mit Sahara-artigen Gürteln.",
            "Até que ponto o ramo descendente da célula de Hadley seca os subtrópicos (em torno de ±26°). A 0,00 os desertos subtropicais desaparecem; valores altos cingem o globo com faixas tipo Sara.",
            "Hadley hücresinin alçalan kolunun subtropikleri ne kadar kuruttuğu (yaklaşık ±26°). 0,00'da subtropik çöller yok olur; yüksek değerler dünyayı Sahra benzeri kuşaklarla sarar.",
        ],
        "Share of oceanic moisture retained in the deepest continental interior. Low values dry out supercontinent hearts into steppes and salt flats; high values let the sea's rain reach far inland." => [
            "Proporción de humedad oceánica que se conserva en el interior continental más profundo. Valores bajos convierten los corazones de los supercontinentes en estepas y salinas; valores altos permiten que la lluvia marina llegue muy tierra adentro.",
            "Part de l'humidité océanique retenue au plus profond de l'intérieur continental. Des valeurs basses transforment le cœur des supercontinents en steppes et salines ; des valeurs hautes laissent la pluie marine pénétrer loin à l'intérieur.",
            "Anteil der ozeanischen Feuchte, der im tiefsten Kontinentinneren erhalten bleibt. Niedrige Werte trocknen die Herzen der Superkontinente zu Steppen und Salzpfannen aus; hohe Werte lassen den Regen des Meeres weit ins Landesinnere reichen.",
            "Proporção de humidade oceânica retida no interior continental mais profundo. Valores baixos transformam os corações dos supercontinentes em estepes e salinas; valores altos deixam a chuva do mar chegar muito para o interior.",
            "En derin kıta içlerinde tutulan okyanus nemi oranı. Düşük değerler süper kıtaların kalbini bozkırlara ve tuz düzlüklerine çevirir; yüksek değerler denizin yağmurunun iç kesimlere kadar ulaşmasını sağlar.",
        ],
        "Multiplier on mountain-induced precipitation. High values create heavy deluges on the windward slopes and severe rain-shadow deserts on the lee." => [
            "Multiplicador de la precipitación inducida por montañas. Valores altos crean diluvios en las laderas de barlovento y severos desiertos de sombra de lluvia en sotavento.",
            "Multiplicateur des précipitations induites par les montagnes. Des valeurs hautes créent de fortes averses sur les versants au vent et de sévères déserts d'ombre pluviométrique sous le vent.",
            "Multiplikator für niederschlagsauslösende Berge. Hohe Werte erzeugen heftige Güsse an den Luvhängen und strenge Regenschattenwüsten im Lee.",
            "Multiplicador da precipitação induzida pelas montanhas. Valores altos criam chuvadas fortes nas encostas de barlavento e desertos severos de sombra de chuva a sotavento.",
            "Dağ kaynaklı yağışın çarpanı. Yüksek değerler rüzgâr üstü yamaçlarda şiddetli sağanaklar, rüzgâr altı yamaçlarda sert yağmur gölgesi çölleri yaratır.",
        ],
        "Share of rainfall that becomes surface river discharge rather than soaking away or evaporating. Higher values mean fuller, wider rivers." => [
            "Proporción de lluvia que se convierte en caudal superficial en lugar de infiltrarse o evaporarse. Valores más altos significan ríos más llenos y anchos.",
            "Part des précipitations qui devient ruissellement de surface au lieu de s'infiltrer ou de s'évaporer. Des valeurs plus hautes signifient des rivières plus pleines et plus larges.",
            "Anteil des Regens, der als Oberflächenabfluss in die Flüsse gelangt, statt zu versickern oder zu verdunsten. Höhere Werte bedeuten vollere, breitere Flüsse.",
            "Proporção de chuva que se torna escoamento superficial em vez de se infiltrar ou evaporar. Valores mais altos significam rios mais cheios e largos.",
            "Sızmak ya da buharlaşmak yerine yüzey akışına dönüşen yağış oranı. Yüksek değerler daha dolu, daha geniş nehirler demektir.",
        ],
        "Smallest discharge, in m³/s, worth drawing as a river. Lower values reveal fine capillary tributaries; higher values display only the great arterial rivers." => [
            "Caudal mínimo, en m³/s, que merece dibujarse como río. Valores bajos revelan finos afluentes capilares; valores altos muestran solo los grandes ríos arteriales.",
            "Débit minimal, en m³/s, qui mérite d'être dessiné comme rivière. Des valeurs basses révèlent de fins affluents capillaires ; des valeurs hautes n'affichent que les grands fleuves artériels.",
            "Kleinster Abfluss in m³/s, der noch als Fluss gezeichnet wird. Niedrige Werte zeigen feine kapillare Nebenflüsse; hohe Werte nur die großen arteriellen Ströme.",
            "Caudal mínimo, em m³/s, que merece ser desenhado como rio. Valores baixos revelam finos afluentes capilares; valores altos mostram apenas os grandes rios arteriais.",
            "Nehir olarak çizilmeye değer en küçük debi (m³/s). Düşük değerler ince kılcal kolları; yüksek değerler yalnızca büyük ana nehirleri gösterir.",
        ],
        "Land provinces across the planet. Drives cell spacing, polygon detail and the fineness of the political boundaries." => [
            "Provincias terrestres del planeta. Determinan el espaciado de celdas, el detalle de los polígonos y la finura de las fronteras políticas.",
            "Provinces terrestres de la planète. Détermine l'espacement des cellules, le détail des polygones et la finesse des frontières politiques.",
            "Landprovinzen auf dem Planeten. Bestimmt Zellabstand, Polygondetail und die Feinheit der politischen Grenzen.",
            "Províncias terrestres do planeta. Determinam o espaçamento das células, o detalhe dos polígonos e a finura das fronteiras políticas.",
            "Gezegendeki kara illeri. Hücre aralığını, poligon ayrıntısını ve siyasi sınırların inceliğini belirler.",
        ],
        "Sea zones partitioned across oceans and gulfs for naval and trade routing." => [
            "Zonas marinas repartidas entre océanos y golfos para el trazado naval y comercial.",
            "Zones marines réparties entre océans et golfes pour le routage naval et commercial.",
            "Seezonen, aufgeteilt auf Ozeane und Golfen für See- und Handelsrouten.",
            "Zonas marinhas repartidas entre oceanos e golfos para o traçado naval e comercial.",
            "Deniz ve ticaret güzergâhları için okyanuslara ve körfezlere bölünmüş deniz bölgeleri.",
        ],
        "Most of the ice-free land that may become impassable wasteland — the deep desert hearts, the glaciated crags, the interior jungle. Set it to 0% for a completely settled planet. Polar ice is not counted: it is geography, not a choice." => [
            "La mayor parte de la tierra libre de hielo que puede volverse yermo intransitable: los corazones de los desiertos profundos, las cumbres glaciares, la selva interior. Ponlo al 0% para un planeta totalmente poblado. El hielo polar no cuenta: es geografía, no una elección.",
            "La plus grande part des terres libres de glace pouvant devenir des terres désolées infranchissables — les cœurs des déserts profonds, les cimes englacées, la jungle intérieure. Mettez 0 % pour une planète entièrement peuplée. La glace polaire ne compte pas : c'est la géographie, pas un choix.",
            "Der größte Teil des eisfreien Landes, das unpassierbares Ödland werden kann — die tiefen Wüstenherzen, die vergletscherten Grate, der Binnen-Dschungel. Bei 0 % ist der Planet vollständig besiedelt. Polareis zählt nicht: Es ist Geografie, keine Entscheidung.",
            "A maior parte da terra livre de gelo que pode tornar-se ermo intransitável — os corações dos desertos profundos, as cristas glaciares, a selva interior. Ponha 0% para um planeta totalmente povoado. O gelo polar não conta: é geografia, não uma escolha.",
            "Geçilemez çoraklığa dönüşebilecek buzsuz karanın çoğu — derin çöl kalpleri, buzullu kayalıklar, iç ormanlar. Tamamen yerleşilmiş bir gezegen için %0 yapın. Kutup buzu sayılmaz: o coğrafyadır, bir seçim değil.",
        ],
        "Environmental hostility above which a province is abandoned to the wild. Lowering it makes the planet harsher and harder to colonise." => [
            "Hostilidad ambiental por encima de la cual una provincia se abandona a la naturaleza. Bajarla hace el planeta más duro y difícil de colonizar.",
            "Hostilité environnementale au-delà de laquelle une province est abandonnée à la nature. L'abaisser rend la planète plus rude et plus difficile à coloniser.",
            "Umweltfeindlichkeit, oberhalb derer eine Provinz der Wildnis überlassen wird. Niedrigere Werte machen den Planeten härter und schwerer zu besiedeln.",
            "Hostilidade ambiental acima da qual uma província é abandonada à natureza. Baixá-la torna o planeta mais duro e difícil de colonizar.",
            "Bir ilin vahşi doğaya bırakıldığı çevresel düşmanlık eşiği. Düşürmek gezegeni daha sert ve kolonileştirmesi daha zor yapar.",
        ],
        "Ceiling on the major continental cores found by the topological clustering. 1 gives a single Pangaean hierarchy, 7 an Earth-like fractured world. Landmasses beyond the ceiling are anchored to the nearest core, exactly as an island is." => [
            "Tope de los núcleos continentales principales hallados por el agrupamiento topológico. 1 da una sola jerarquía pangéica; 7, un mundo fragmentado como la Tierra. Las masas de tierra más allá del tope se anclan al núcleo más cercano, igual que una isla.",
            "Plafond des principaux cœurs continentaux trouvés par le regroupement topologique. 1 donne une hiérarchie pangéenne unique, 7 un monde fracturé comme la Terre. Les masses terrestres au-delà du plafond sont ancrées au cœur le plus proche, exactement comme une île.",
            "Obergrenze der großen kontinentalen Kerne, die die topologische Gruppierung findet. 1 ergibt eine einzige pangäische Hierarchie, 7 eine erdähnlich zersplitterte Welt. Landmassen jenseits der Grenze werden an den nächsten Kern verankert, genau wie eine Insel.",
            "Limite dos principais núcleos continentais encontrados pelo agrupamento topológico. 1 dá uma única hierarquia pangeia; 7, um mundo fraturado como a Terra. As massas de terra além do limite ancoram-se ao núcleo mais próximo, tal como uma ilha.",
            "Topolojik kümelemenin bulduğu büyük kıta çekirdeklerinin üst sınırı. 1 tek bir Pangea hiyerarşisi, 7 Dünya benzeri parçalanmış bir dünya verir. Sınırın ötesindeki kara kütleleri, tıpkı bir ada gibi en yakın çekirdeğe bağlanır.",
        ],
        "N" => ["N", "N", "N", "N", "K"],
        "S" => ["S", "S", "S", "S", "G"],
        "E" => ["E", "E", "O", "L", "D"],
        "W" => ["O", "O", "W", "O", "B"],
        // ── The eight acts of creation ───────────────────────────────────────
        "Geology & Erosion" => [
            "Geología y erosión",
            "Géologie et érosion",
            "Geologie und Erosion",
            "Geologia e erosão",
            "Jeoloji ve Erozyon",
        ],
        "Atmospheric Climate" => [
            "Clima atmosférico",
            "Climat atmosphérique",
            "Atmosphärisches Klima",
            "Clima atmosférico",
            "Atmosferik İklim",
        ],
        "Hydrology & Rivers" => [
            "Hidrología y ríos",
            "Hydrologie et rivières",
            "Hydrologie und Flüsse",
            "Hidrologia e rios",
            "Hidroloji ve Nehirler",
        ],
        "Geography & Provinces" => [
            "Geografía y provincias",
            "Géographie et provinces",
            "Geografie und Provinzen",
            "Geografia e províncias",
            "Coğrafya ve İller",
        ],
        "Geopolitical Hierarchy" => [
            "Jerarquía geopolítica",
            "Hiérarchie géopolitique",
            "Geopolitische Hierarchie",
            "Hierarquia geopolítica",
            "Jeopolitik Hiyerarşi",
        ],
        "Hydro-Political Hierarchy" => [
            "Jerarquía hidropolítica",
            "Hiérarchie hydro-politique",
            "Hydro-politische Hierarchie",
            "Hierarquia hidropolítica",
            "Hidro-Politik Hiyerarşi",
        ],
        "Dawn of Civilization" => [
            "Amanecer de la civilización",
            "Aube de la civilisation",
            "Morgenröte der Zivilisation",
            "Alvor da civilização",
            "Uygarlığın Şafağı",
        ],
        "Languages & Phonetics" => [
            "Idiomas y fonética",
            "Langues et phonétique",
            "Sprachen und Phonetik",
            "Línguas e fonética",
            "Diller ve Sesbilim",
        ],
        "Tectonic plates, fractal detail and hydraulic + thermal erosion: the raw land." => [
            "Placas tectónicas, detalle fractal y erosión hídrica y térmica: la tierra en bruto.",
            "Plaques tectoniques, détail fractal et érosion hydraulique et thermique : la terre brute.",
            "Tektonische Platten, fraktale Details und hydraulische sowie thermische Erosion: das rohe Land.",
            "Placas tectónicas, detalhe fractal e erosão hídrica e térmica: a terra em bruto.",
            "Tektonik levhalar, fraktal ayrıntı ve hidrolik ile termal erozyon: ham toprak.",
        ],
        "Insolation, circulation and rain shadows: temperature, winds and rainfall." => [
            "Insolación, circulación y sombras de lluvia: temperatura, vientos y precipitación.",
            "Insolation, circulation et ombres pluviométriques : température, vents et précipitations.",
            "Einstrahlung, Zirkulation und Regenschatten: Temperatur, Winde und Niederschlag.",
            "Insolação, circulação e sombras de chuva: temperatura, ventos e precipitação.",
            "Güneşlenme, dolaşım ve yağmur gölgeleri: sıcaklık, rüzgârlar ve yağış.",
        ],
        "Depressions fill into lakes and rainfall gathers into rivers. The world cache is written here." => [
            "Las depresiones se llenan de lagos y la lluvia se reúne en ríos. Aquí se escribe la caché del mundo.",
            "Les dépressions se remplissent de lacs et les pluies se rassemblent en rivières. Le cache du monde s'écrit ici.",
            "Senken füllen sich zu Seen, und Regen sammelt sich in Flüssen. Hier wird der Welt-Cache geschrieben.",
            "As depressões enchem-se de lagos e a chuva junta-se em rios. É aqui que a cache do mundo é escrita.",
            "Çukurlar göllere dolar, yağmur nehirlere toplanır. Dünya önbelleği burada yazılır.",
        ],
        "The inhabited land divided into provinces, its wilderness mapped." => [
            "La tierra habitada dividida en provincias y su naturaleza salvaje cartografiada.",
            "La terre habitée divisée en provinces, ses étendues sauvages cartographiées.",
            "Das bewohnte Land in Provinzen geteilt, seine Wildnis kartiert.",
            "A terra habitada dividida em províncias e a sua natureza selvagem cartografada.",
            "Yerleşik toprak illere bölünür, vahşi doğası haritalanır.",
        ],
        "Provinces gathered into areas, regions, superregions and continents." => [
            "Provincias agrupadas en áreas, regiones, superregiones y continentes.",
            "Provinces rassemblées en zones, régions, superrégions et continents.",
            "Provinzen zu Gebieten, Regionen, Superregionen und Kontinenten zusammengefasst.",
            "Províncias reunidas em áreas, regiões, super-regiões e continentes.",
            "İller bölgelere, yörelere, üst yörelere ve kıtalara toplanır.",
        ],
        "The world ocean partitioned into its own five matching tiers." => [
            "El océano mundial dividido en sus propios cinco niveles equivalentes.",
            "L'océan mondial partagé en ses propres cinq niveaux correspondants.",
            "Der Welttozean in seine eigenen fünf passenden Stufen gegliedert.",
            "O oceano mundial dividido nos seus próprios cinco níveis correspondentes.",
            "Dünya okyanusu kendi beş eşleşen düzeyine bölünür.",
        ],
        "Realms rise, spread and are given a history, an age and a people." => [
            "Los reinos surgen, se expanden y reciben una historia, una era y un pueblo.",
            "Des royaumes naissent, s'étendent et reçoivent une histoire, un âge et un peuple.",
            "Reiche entstehen, breiten sich aus und erhalten eine Geschichte, ein Zeitalter und ein Volk.",
            "Reinos surgem, expandem-se e recebem uma história, uma era e um povo.",
            "Krallıklar doğar, yayılır ve bir tarih, bir çağ ve bir halk alır.",
        ],
        "Phonologies, proto-lexicons, Urheimats and the tongues that spread from them." => [
            "Fonologías, protolexicones, Urheimats y las lenguas que se extienden desde ellos.",
            "Phonologies, protolexiques, Urheimats et les langues qui en rayonnent.",
            "Phonologien, Protowortschätze, Urheimaten und die Sprachen, die von ihnen ausstrahlen.",
            "Fonologias, proto-léxicos, Urheimats e as línguas que deles se espalham.",
            "Sesbilimler, ön sözlükler, Urheimat'lar ve onlardan yayılan diller.",
        ],
        "Hunter-gatherers" => [
            "Cazadores-recolectores",
            "Chasseurs-cueilleurs",
            "Jäger und Sammler",
            "Caçadores-recolectores",
            "Avcı-toplayıcılar",
        ],
        "Nomadic pastoralists" => [
            "Pastores nómadas",
            "Pasteurs nomades",
            "Nomadische Hirten",
            "Pastores nómadas",
            "Göçebe çobanlar",
        ],
        "Simple farming societies" => [
            "Sociedades agrícolas simples",
            "Sociétés agricoles simples",
            "Einfache Agrargesellschaften",
            "Sociedades agrícolas simples",
            "Basit tarım toplumları",
        ],
        "Complex farming societies" => [
            "Sociedades agrícolas complejas",
            "Sociétés agricoles complexes",
            "Komplexe Agrargesellschaften",
            "Sociedades agrícolas complexas",
            "Karmaşık tarım toplumları",
        ],
        "State societies" => [
            "Sociedades estatales",
            "Sociétés étatiques",
            "Staatsgesellschaften",
            "Sociedades estatais",
            "Devlet toplumları",
        ],
        "A country of bands: a few families read one valley, move with the season, and answer to nobody but their own elders." => [
            "Un país de bandas: unas pocas familias leen un valle, se mueven con la estación y no responden ante nadie salvo sus propios ancianos.",
            "Un pays de bandes : quelques familles lisent une vallée, suivent la saison et ne répondent qu'à leurs propres anciens.",
            "Ein Land der Verbände: wenige Familien lesen ein Tal, ziehen mit der Jahreszeit und sind niemandem außer ihren eigenen Ältesten Rechenschaft schuldig.",
            "Um país de bandos: poucas famílias leem um vale, movem-se com a estação e não respondem perante ninguém além dos seus anciãos.",
            "Kabileler ülkesi: birkaç aile bir vadiyi okur, mevsimle göç eder ve kendi büyüklerinden başkasına hesap vermez.",
        ],
        "A country of herders: wealth is counted in animals and measured in pasture, so the people move and the horizon is their border." => [
            "Un país de pastores: la riqueza se cuenta en animales y se mide en pastos, así que el pueblo se mueve y el horizonte es su frontera.",
            "Un pays d'éleveurs : la richesse se compte en bêtes et se mesure en pâturages ; le peuple se déplace et l'horizon est sa frontière.",
            "Ein Land der Hirten: Reichtum wird in Tieren gezählt und in Weideland gemessen, also zieht das Volk, und der Horizont ist seine Grenze.",
            "Um país de pastores: a riqueza conta-se em animais e mede-se em pastagem, por isso o povo move-se e o horizonte é a sua fronteira.",
            "Çobanlar ülkesi: servet hayvanla sayılır, otlakla ölçülür; halk göç eder ve ufuk onların sınırıdır.",
        ],
        "A country of villages: fields are cleared and kept, the household is the unit of work, and no rank stands above it." => [
            "Un país de aldeas: los campos se desbrozan y se conservan, el hogar es la unidad de trabajo y ningún rango está por encima de él.",
            "Un pays de villages : les champs sont défrichés et gardés, le foyer est l'unité de travail, et nul rang ne s'élève au-dessus.",
            "Ein Land der Dörfer: Felder werden gerodet und gehalten, der Haushalt ist die Arbeitseinheit, und kein Rang steht darüber.",
            "Um país de aldeias: os campos são desbravados e mantidos, o agregado é a unidade de trabalho e nenhum posto está acima dele.",
            "Köyler ülkesi: tarlalar açılır ve korunur, çalışma birimi hane halkıdır ve onun üstünde hiçbir makam yoktur.",
        ],
        "A ranked farming country: chiefs and lineages hold the land, tribute flows upward, and a great man is buried with more than he could use." => [
            "Un país agrícola jerarquizado: jefes y linajes poseen la tierra, el tributo fluye hacia arriba y un gran hombre es enterrado con más de lo que podía usar.",
            "Un pays agricole hiérarchisé : chefs et lignages tiennent la terre, le tribut remonte, et un grand homme est enterré avec plus qu'il ne pouvait user.",
            "Ein ranggeordnetes Ackerland: Häuptlinge und Linien halten das Land, Tribut fließt nach oben, und ein großer Mann wird mit mehr begraben, als er verbrauchen konnte.",
            "Um país agrícola hierarquizado: chefes e linhagens detêm a terra, o tributo flui para cima, e um grande homem é enterrado com mais do que poderia usar.",
            "Sınıflı bir tarım ülkesi: reisler ve soylar toprağı tutar, haraç yukarı akar ve büyük bir adam kullanabileceğinden fazlasıyla gömülür.",
        ],
        "A state: offices, archives and a standing apparatus, which can tax a harvest it never touched and raise a wall it will not live to see finished." => [
            "Un estado: cargos, archivos y un aparato permanente, capaz de gravar una cosecha que nunca tocó y levantar una muralla que no verá terminar.",
            "Un État : des offices, des archives et un appareil permanent, capable de taxer une récolte qu'il n'a jamais touchée et d'élever une muraille qu'il ne verra pas finir.",
            "Ein Staat: Ämter, Archive und ein stehender Apparat, der eine Ernte besteuern kann, die er nie berührt hat, und eine Mauer errichtet, deren Vollendung er nicht erleben wird.",
            "Um estado: cargos, arquivos e um aparelho permanente, capaz de taxar uma colheita que nunca tocou e erguer uma muralha que não verá terminada.",
            "Bir devlet: makamlar, arşivler ve daimi bir aygıt; hiç dokunmadığı bir hasadı vergilendirebilir ve bitişini göremeyeceği bir sur inşa edebilir.",
        ],
        _ => return None,
    };
    Some(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalog's own source, so the completeness check reads the table that
    /// actually ships rather than a hand-kept list that could drift from it.
    const SOURCE: &str = include_str!("i18n.rs");

    /// Every key in the catalog, unescaped, in source order.
    fn catalog_keys() -> Vec<String> {
        SOURCE
            .lines()
            .filter_map(|line| {
                let key = line.strip_prefix("        \"")?;
                let key = key.strip_suffix("\" => [")?;
                Some(key.replace("\\n", "\n").replace("\\\"", "\""))
            })
            .collect()
    }

    #[test]
    fn every_row_is_complete_and_keeps_its_placeholders() {
        let keys = catalog_keys();
        assert!(keys.len() > 150, "only {} keys were found", keys.len());

        let mut seen = std::collections::HashSet::new();
        for key in &keys {
            assert!(seen.insert(key.clone()), "duplicate key {key:?}");
            let row = catalog(key).unwrap_or_else(|| panic!("no row for {key:?}"));
            let expected = key.matches("{}").count();
            for (index, translation) in row.iter().enumerate() {
                assert!(
                    !translation.trim().is_empty(),
                    "{key:?} column {index} is empty"
                );
                assert_eq!(
                    translation.matches("{}").count(),
                    expected,
                    "{key:?} column {index} ({translation:?}) has the wrong number of placeholders"
                );
            }
        }
    }

    #[test]
    fn english_is_returned_untouched_and_unknown_lines_fall_back() {
        assert_eq!(
            Language::English.text("🌍 Atlas Viewer"),
            "🌍 Atlas Viewer"
        );
        assert_eq!(
            Language::Spanish.text("🌍 Atlas Viewer"),
            "🌍 Atlas del Mundo"
        );
        // A line nobody has translated yet reads as English, never as a marker.
        assert_eq!(
            Language::Turkish.text("Not a line in the catalog"),
            "Not a line in the catalog"
        );
    }

    #[test]
    fn a_template_places_its_values_where_the_translation_puts_the_braces() {
        let filled = Language::German.fill(
            "Tectonics: {} plates · {}% ocean · {} erosion droplets",
            &[&12, &71, &3_500_000],
        );
        assert_eq!(
            filled,
            "Tektonik: 12 Platten · 71 % Ozean · 3500000 Erosionstropfen"
        );

        // Turkish moves the per-cent sign in front of the figure.
        assert_eq!(
            Language::Turkish.fill("🌍 Surface: {}% ocean", &[&71]),
            "🌍 Yüzey: %71 okyanus"
        );

        // A template with fewer arguments than braces never panics.
        assert_eq!(fill_template("{} and {}", &[&"one"]), "one and {}");
        assert_eq!(fill_template("no braces", &[&"one"]), "no braces");
    }

    #[test]
    fn a_locale_tag_is_read_down_to_its_language() {
        assert_eq!(Language::from_tag("es"), Language::Spanish);
        assert_eq!(Language::from_tag("es_ES.UTF-8"), Language::Spanish);
        assert_eq!(Language::from_tag("pt-BR"), Language::Portuguese);
        assert_eq!(Language::from_tag("tr_TR"), Language::Turkish);
        assert_eq!(Language::from_tag("de-DE"), Language::German);
        assert_eq!(Language::from_tag("fr_FR.UTF-8"), Language::French);
        assert_eq!(Language::from_tag("en_GB"), Language::English);
        assert_eq!(Language::from_tag("C"), Language::English);
        assert_eq!(Language::from_tag("POSIX"), Language::English);
        assert_eq!(Language::from_tag("ja_JP"), Language::English);
        assert_eq!(Language::from_tag(""), Language::English);

        // The command line only accepts a language the atlas actually speaks.
        assert_eq!(Language::from_known_tag("de-DE"), Some(Language::German));
        assert_eq!(Language::from_known_tag("pt_BR.UTF-8"), Some(Language::Portuguese));
        assert_eq!(Language::from_known_tag("ja_JP"), None);
        assert_eq!(Language::from_known_tag("klingon"), None);
    }

    #[test]
    fn every_language_has_a_code_and_a_native_name() {
        for language in Language::ALL {
            assert!(!language.code().is_empty());
            assert!(!language.native_name().is_empty());
            assert_eq!(Language::from_tag(language.code()), language);
        }
    }

    #[test]
    fn a_chosen_language_is_remembered_beside_the_worlds() {
        let dir = std::env::temp_dir().join(format!(
            "genesis_language_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |t| t.subsec_nanos())
        ));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(Language::load_preference(&dir), None);

        Language::Turkish.save_preference(&dir);
        assert_eq!(Language::load_preference(&dir), Some(Language::Turkish));

        // An unreadable preference is "no preference", never a failed start.
        std::fs::write(dir.join(PREFERENCE_FILE), "klingon").unwrap();
        assert_eq!(Language::load_preference(&dir), None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

