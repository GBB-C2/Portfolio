//! Small exact-match UI catalogue. User-owned names and paths are passed through
//! the raw text path; template captures are inserted once without substitution.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Language {
    #[default]
    Russian,
    English,
    German,
    Korean,
    Chinese,
    Japanese,
}

impl Language {
    pub const ALL: [Self; 6] = [
        Self::Russian,
        Self::English,
        Self::German,
        Self::Korean,
        Self::Chinese,
        Self::Japanese,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Russian => "ru",
            Self::English => "en",
            Self::German => "de",
            Self::Korean => "ko",
            Self::Chinese => "zh",
            Self::Japanese => "ja",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Russian => "Русский",
            Self::English => "English",
            Self::German => "Deutsch",
            Self::Korean => "한국어",
            Self::Chinese => "简体中文",
            Self::Japanese => "日本語",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "ru" | "ru-ru" | "russian" => Some(Self::Russian),
            "en" | "en-us" | "en-gb" | "english" => Some(Self::English),
            "de" | "de-de" | "german" => Some(Self::German),
            "ko" | "ko-kr" | "korean" => Some(Self::Korean),
            "zh" | "zh-cn" | "zh-hans" | "chinese" => Some(Self::Chinese),
            "ja" | "ja-jp" | "japanese" => Some(Self::Japanese),
            _ => None,
        }
    }

    pub fn translate(self, source: &str) -> String {
        let index = match self {
            Self::Russian => return source.to_owned(),
            Self::English => 0,
            Self::German => 1,
            Self::Korean => 2,
            Self::Chinese => 3,
            Self::Japanese => 4,
        };
        if let Some((_, translated)) = CATALOG.iter().find(|(key, _)| *key == source) {
            return translated[index].to_owned();
        }
        for template in TEMPLATES {
            if let Some(values) = capture(template.source, source) {
                let values: Vec<String> = values
                    .into_iter()
                    .enumerate()
                    .map(|(i, value)| {
                        if template.ui_slots & (1 << i) != 0 {
                            // Only exact catalogue entries may translate a UI-label slot;
                            // arbitrary file names and nested formatting are never evaluated.
                            CATALOG
                                .iter()
                                .find(|(key, _)| *key == value)
                                .map(|(_, text)| text[index].to_owned())
                                .unwrap_or_else(|| value.to_owned())
                        } else {
                            value.to_owned()
                        }
                    })
                    .collect();
                return substitute(template.translated[index], &values);
            }
        }
        source.to_owned()
    }
}

struct Template {
    source: &'static str,
    translated: [&'static str; 5],
    ui_slots: u8,
}

// Templates use numbered, non-repeating captures separated by fixed text.
fn capture<'a>(pattern: &str, source: &'a str) -> Option<Vec<&'a str>> {
    let mut rest = source;
    let mut pattern_rest = pattern;
    let mut result = Vec::new();
    while let Some(start) = pattern_rest.find('{') {
        rest = rest.strip_prefix(&pattern_rest[..start])?;
        let end = pattern_rest[start..].find('}')? + start;
        let slot: usize = pattern_rest[start + 1..end].parse().ok()?;
        if slot != result.len() {
            return None;
        }
        pattern_rest = &pattern_rest[end + 1..];
        let next_slot = pattern_rest.find('{').unwrap_or(pattern_rest.len());
        let separator = &pattern_rest[..next_slot];
        if next_slot == pattern_rest.len() {
            let value = rest.strip_suffix(separator)?;
            result.push(value);
            rest = &rest[rest.len()..];
            pattern_rest = "";
            break;
        }
        if separator.is_empty() {
            return None;
        }
        let index = rest.find(separator)?;
        result.push(&rest[..index]);
        rest = &rest[index..];
    }
    if rest != pattern_rest {
        return None;
    }
    Some(result)
}

fn substitute(pattern: &str, values: &[String]) -> String {
    let mut output = String::new();
    let mut rest = pattern;
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}').map(|n| n + start) else {
            break;
        };
        let Ok(index) = rest[start + 1..end].parse::<usize>() else {
            break;
        };
        let Some(value) = values.get(index) else {
            break;
        };
        output.push_str(&rest[..start]);
        output.push_str(value);
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    output
}

// Locale order: English, German, Korean, Simplified Chinese, Japanese.
const CATALOG: &[(&str, [&str; 5])] = &[
    ("Щетина", ["Bristle", "Borstenpinsel", "강모", "鬃毛笔", "剛毛ブラシ"]),
    ("Пунктирная", ["Stipple", "Tupfenpinsel", "점묘", "点描笔", "点描ブラシ"]),
    ("Сухая кисть", ["Dry brush", "Trockenpinsel", "마른 붓", "干画笔", "ドライブラシ"]),
    ("Двойное перо", ["Twin nib", "Doppelfeder", "이중 펜촉", "双头笔", "二重ペン"]),
    ("Щетинный", ["Bristle eraser", "Borstenradierer", "강모 지우개", "鬃毛橡皮擦", "剛毛消しゴム"]),
    ("Точечный", ["Stipple eraser", "Punktradierer", "점묘 지우개", "点描橡皮擦", "点描消しゴム"]),
    ("Сухой", ["Dry eraser", "Trockenradierer", "마른 지우개", "干擦", "ドライ消しゴム"]),
    ("Двойной", ["Twin eraser", "Doppelradierer", "이중 지우개", "双头橡皮擦", "二重消しゴム"]),
    ("Треугольник", ["Triangle", "Dreieck", "삼각형", "三角形", "三角形"]),
    ("Ромб", ["Diamond", "Raute", "마름모", "菱形", "ひし形"]),
    ("Пятиугольник", ["Pentagon", "Fünfeck", "오각형", "五边形", "五角形"]),
    ("Шестиугольник", ["Hexagon", "Sechseck", "육각형", "六边形", "六角形"]),
    ("Звезда", ["Star", "Stern", "별", "星形", "星形"]),
    ("Масштаб фактуры", ["Texture scale", "Texturgröße", "질감 크기", "纹理缩放", "テクスチャの倍率"]),
    ("Разброс непрозрачности", ["Opacity jitter", "Deckkraftstreuung", "불투명도 변동", "不透明度抖动", "不透明度の揺らぎ"]),
    ("Непрозрачность заливки", ["Fill opacity", "Fülldeckkraft", "채우기 불투명도", "填充不透明度", "塗りつぶしの不透明度"]),
    ("Направление тени", ["Shadow direction", "Schattenrichtung", "그림자 방향", "阴影方向", "影の方向"]),
    ("Подчёркивание", ["Underline", "Unterstreichen", "밑줄", "下划线", "下線"]),
    ("Зачёркивание", ["Strikethrough", "Durchstreichen", "취소선", "删除线", "取り消し線"]),
    ("Выбор шрифта", ["Choose typeface", "Schrift wählen", "글꼴 선택", "选择字体", "フォントの選択"]),
    ("Поиск шрифта", ["Search fonts", "Schrift suchen", "글꼴 검색", "搜索字体", "フォントを検索"]),
    ("Шрифты не найдены", ["No matching fonts", "Keine Schriften gefunden", "검색된 글꼴 없음", "未找到匹配的字体", "一致するフォントがありません"]),
    ("Прокрутка шрифтов", ["Scroll fonts", "Schriftliste scrollen", "글꼴 목록 스크롤", "滚动字体列表", "フォント一覧をスクロール"]),
    ("Округлость", ["Roundness", "Rundheit", "원형률", "圆度", "真円率"]),
    ("Зерно", ["Grain", "Körnung", "입자", "颗粒", "粒子"]),
    ("Штрих", ["Stroke", "Strich", "획", "笔画", "ストローク"]),
    ("Переименовать кисть", ["Rename brush", "Pinsel umbenennen", "브러시 이름 변경", "重命名画笔", "ブラシ名を変更"]),
    ("Создать копию кисти", ["Duplicate brush", "Pinsel duplizieren", "브러시 복제", "复制画笔", "ブラシを複製"]),
    ("Переименовать ластик", ["Rename eraser", "Radierer umbenennen", "지우개 이름 변경", "重命名橡皮擦", "消しゴム名を変更"]),
    ("Создать копию ластика", ["Duplicate eraser", "Radierer duplizieren", "지우개 복제", "复制橡皮擦", "消しゴムを複製"]),
    ("Сохранить название", ["Save name", "Namen speichern", "이름 저장", "保存名称", "名前を保存"]),
    ("Не удалось сохранить кисти.", ["Could not save brushes.", "Pinsel konnten nicht gespeichert werden.", "브러시를 저장할 수 없습니다.", "无法保存画笔。", "ブラシを保存できませんでした。"]),
    ("Текст · T", ["Text · T", "Text · T", "텍스트 · T", "文本 · T", "テキスト · T"]),
    ("Редактировать надпись", ["Edit text", "Text bearbeiten", "텍스트 편집", "编辑文本", "テキストを編集"]),
    ("Размер шрифта", ["Font size", "Schriftgröße", "글꼴 크기", "字号", "フォントサイズ"]),
    ("Шрифт", ["Font", "Schrift", "글꼴", "字体", "フォント"]),
    ("Начертание", ["Style", "Schriftschnitt", "글꼴 스타일", "字形", "書体"]),
    ("Цветовой круг", ["Color disc", "Farbkreis", "색상환", "色盘", "カラーディスク"]),
    ("Насыщенность и яркость", ["Saturation and brightness", "Sättigung und Helligkeit", "채도 및 밝기", "饱和度和亮度", "彩度と明度"]),
    ("Цвет текста", ["Text color", "Textfarbe", "텍스트 색상", "文字颜色", "文字色"]),
    ("Позиция", ["Position", "Position", "위치", "位置", "位置"]),
    ("Предыдущий шрифт", ["Previous font", "Vorherige Schrift", "이전 글꼴", "上一种字体", "前のフォント"]),
    ("Следующий шрифт", ["Next font", "Nächste Schrift", "다음 글꼴", "下一种字体", "次のフォント"]),
    ("Новая надпись", ["New text", "Neuer Text", "새 텍스트", "新建文字", "新しいテキスト"]),
    ("Щёлкните по холсту для новой надписи.", ["Click the canvas to place new text.", "Für neuen Text auf die Leinwand klicken.", "캔버스를 클릭해 새 텍스트를 배치하세요.", "单击画布放置新文字。", "キャンバスをクリックして文字を配置します。"]),
    ("Координата в пикселях", ["Coordinate in pixels", "Koordinate in Pixeln", "픽셀 좌표", "像素坐标", "ピクセル座標"]),
    ("Оттенок", ["Hue", "Farbton", "색조", "色相", "色相"]),
    ("Достигнут предел слоёв.", ["Layer limit reached.", "Ebenenlimit erreicht.", "레이어 한도에 도달했습니다.", "已达到图层上限。", "レイヤー数の上限に達しました。"]),
    ("Обычный", ["Regular", "Standard", "기본", "常规", "標準"]),
    ("Подпись", ["Caption", "Bildunterschrift", "캡션", "题注", "キャプション"]),
    ("Заголовок", ["Headline", "Überschrift", "제목", "标题", "見出し"]),
    ("Афиша", ["Poster", "Plakat", "포스터", "海报", "ポスター"]),
    ("Рукопись", ["Manuscript", "Handschrift", "손글씨", "手写体", "手書き"]),
    ("Автограф", ["Signature", "Signatur", "서명", "签名", "サイン"]),
    ("Контур", ["Outline", "Kontur", "윤곽선", "描边", "縁取り"]),
    ("С тенью", ["Drop shadow", "Schlagschatten", "그림자", "投影", "影付き"]),
    ("Объёмный", ["Extruded", "Extrudiert", "돌출", "立体字", "立体文字"]),
    ("Рельеф", ["Relief", "Relief", "부조", "浮雕", "レリーフ"]),
    ("Для чтения", ["Body text", "Lesetext", "본문", "正文", "本文"]),
    ("Заголовки", ["Headlines", "Überschriften", "제목", "标题样式", "見出し"]),
    ("Рукописные", ["Handwritten", "Handschriften", "필기체", "手写", "手書き体"]),
    ("Эффекты", ["Effects", "Effekte", "효과", "效果", "効果"]),
    ("Объём", ["Volume", "Volumen", "입체", "立体", "立体"]),
    ("Параметры текста · F4", ["Text settings · F4", "Texteinstellungen · F4", "텍스트 설정 · F4", "文字设置 · F4", "テキスト設定 · F4"]),
    ("Поиск стилей текста", ["Search text styles", "Textstile suchen", "텍스트 스타일 검색", "搜索文字样式", "文字スタイルを検索"]),
    ("Оформление текста", ["Text formatting", "Textformatierung", "텍스트 서식", "文字格式", "文字の書式"]),
    ("Толщина обводки", ["Outline width", "Konturstärke", "윤곽선 두께", "描边宽度", "縁取り幅"]),
    ("Смещение тени", ["Shadow offset", "Schattenversatz", "그림자 거리", "阴影偏移", "影の距離"]),
    ("Непрозрачность тени", ["Shadow opacity", "Schattendeckkraft", "그림자 불투명도", "阴影不透明度", "影の不透明度"]),
    ("Глубина 3D", ["3D depth", "3D-Tiefe", "3D 깊이", "3D 深度", "3D の深さ"]),
    ("Направление объёма", ["Extrusion angle", "Extrusionswinkel", "돌출 방향", "立体方向", "押し出し角度"]),
    ("Цвет эффекта", ["Effect color", "Effektfarbe", "효과 색상", "效果颜色", "効果色"]),
    ("Текст на холсте", ["Canvas text", "Text auf der Leinwand", "캔버스 텍스트", "画布文本", "キャンバスのテキスト"]),
    ("Щёлкните по холсту для надписи или по буквам для правки.", ["Click the canvas to add text or click lettering to edit.", "Zum Schreiben auf die Leinwand oder zum Bearbeiten auf Buchstaben klicken.", "캔버스를 클릭해 텍스트를 추가하거나 글자를 클릭해 편집하세요.", "单击画布添加文本，单击文字进行编辑。", "キャンバスをクリックして追加、文字をクリックして編集します。"]),
    ("Для рисования выберите растровый слой.", ["Select a raster layer to paint.", "Zum Malen eine Rasterebene wählen.", "그리려면 래스터 레이어를 선택하세요.", "请选择栅格图层进行绘画。", "描画するにはラスター層を選択してください。"]),
    ("Выберите текстовый слой.", ["Select a text layer.", "Eine Textebene auswählen.", "텍스트 레이어를 선택하세요.", "请选择文本图层。", "テキスト層を選択してください。"]),
    ("Введите текст длиной до 4096 знаков.", ["Enter up to 4096 characters of text.", "Text mit bis zu 4096 Zeichen eingeben.", "4096자 이내의 텍스트를 입력하세요.", "请输入最多 4096 个字符。", "4096文字以内で入力してください。"]),
    ("Проверьте шрифт, размер и координаты текста.", ["Check the font, size and text position.", "Schrift, Größe und Textposition prüfen.", "글꼴, 크기 및 위치를 확인하세요.", "请检查字体、字号和文本位置。", "フォント、サイズ、座標を確認してください。"]),
    ("Неизвестная версия файла Portfolio.", ["Unsupported Portfolio file version.", "Unbekannte Portfolio-Dateiversion.", "지원되지 않는 Portfolio 파일 버전입니다.", "不支持的 Portfolio 文件版本。", "未対応の Portfolio ファイル形式です。"]),
    ("Размер (px)", ["Size (px)", "Größe (px)", "크기 (px)", "字号 (px)", "サイズ (px)"]),
    ("Цвет (HEX)", ["Color (HEX)", "Farbe (HEX)", "색상 (HEX)", "颜色 (HEX)", "色 (HEX)"]),
    ("Полужирный", ["Bold", "Fett", "굵게", "粗体", "太字"]),
    ("Курсив", ["Italic", "Kursiv", "기울임꼴", "斜体", "斜体"]),
    ("Выравнивание", ["Alignment", "Ausrichtung", "정렬", "对齐", "配置"]),
    ("Слева", ["Left", "Links", "왼쪽", "左对齐", "左揃え"]),
    ("По центру", ["Center", "Zentriert", "가운데", "居中", "中央揃え"]),
    ("Справа", ["Right", "Rechts", "오른쪽", "右对齐", "右揃え"]),
    ("Введите размер от 4 до 512 px.", ["Enter a size from 4 to 512 px.", "Größe von 4 bis 512 px eingeben.", "4~512px 크기를 입력하세요.", "请输入 4 到 512 px 的字号。", "4～512 px のサイズを入力してください。"]),
    ("Координата должна находиться внутри холста.", ["Choose a position inside the canvas.", "Eine Position innerhalb der Leinwand wählen.", "캔버스 안의 위치를 선택하세요.", "请选择画布内的位置。", "キャンバス内の座標を指定してください。"]),
    ("Не удалось отрисовать текст.", ["Could not render the text.", "Text konnte nicht gerendert werden.", "텍스트를 렌더링할 수 없습니다.", "无法渲染文本。", "テキストを描画できませんでした。"]),
    ("Текст выходит за границы холста. Измените координаты или размер.", ["The text falls outside the canvas. Adjust its position or size.", "Der Text liegt außerhalb der Leinwand. Position oder Größe ändern.", "텍스트가 캔버스를 벗어났습니다. 위치나 크기를 조정하세요.", "文本超出画布。请调整位置或字号。", "文字がキャンバス外にあります。位置かサイズを変えてください。"]),
    ("Цвет · открыть палитру", ["Color · open palette", "Farbe · Palette öffnen", "색상 · 팔레트 열기", "颜色 · 打开调色板", "色 · パレットを開く"]),
    ("Кисть", ["Brush", "Pinsel", "브러시", "画笔", "ブラシ"]),
    ("Ластик", ["Eraser", "Radierer", "지우개", "橡皮擦", "消しゴム"]),
    ("Заливка", ["Fill", "Füllen", "채우기", "填充", "塗りつぶし"]),
    ("Пипетка", ["Eyedropper", "Pipette", "스포이트", "吸管", "スポイト"]),
    ("Линия", ["Line", "Linie", "선", "直线", "直線"]),
    ("Прямоугольник", ["Rectangle", "Rechteck", "사각형", "矩形", "長方形"]),
    ("Эллипс", ["Ellipse", "Ellipse", "타원", "椭圆", "楕円"]),
    ("Перемещение", ["Pan", "Verschieben", "이동", "平移", "移動"]),
    ("Файл", ["File", "Datei", "파일", "文件", "ファイル"]),
    ("Отменить · Ctrl+Z", ["Undo · Ctrl+Z", "Rückgängig · Ctrl+Z", "실행 취소 · Ctrl+Z", "撤销 · Ctrl+Z", "元に戻す · Ctrl+Z"]),
    ("Повторить · Ctrl+Shift+Z", ["Redo · Ctrl+Shift+Z", "Wiederholen · Ctrl+Shift+Z", "다시 실행 · Ctrl+Shift+Z", "重做 · Ctrl+Shift+Z", "やり直す · Ctrl+Shift+Z"]),
    ("Тема «Бумага»", ["Paper theme", "Design Papier", "종이 테마", "纸张主题", "ペーパーテーマ"]),
    ("Тема «Графит»", ["Graphite theme", "Design Graphit", "흑연 테마", "石墨主题", "グラファイトテーマ"]),
    ("Настройки внешнего вида · Ctrl+,", ["Appearance settings · Ctrl+,", "Darstellung · Ctrl+,", "화면 설정 · Ctrl+,", "外观设置 · Ctrl+,", "外観設定 · Ctrl+,"]),
    ("Экспорт", ["Export", "Export", "내보내기", "导出", "書き出し"]),
    ("Сохраняем…", ["Saving…", "Wird gespeichert…", "저장 중…", "正在保存…", "保存中…"]),
    ("Инструменты и масштаб", ["Tools and zoom", "Werkzeuge und Zoom", "도구 및 확대/축소", "工具与缩放", "ツールとズーム"]),
    ("Кисть · B · нажмите для выбора", ["Brush · B · choose a preset", "Pinsel · B · Variante wählen", "브러시 · B · 프리셋 선택", "画笔 · B · 选择预设", "ブラシ · B · プリセットを選択"]),
    ("Ластик · E · нажмите для выбора", ["Eraser · E · choose a preset", "Radierer · E · Variante wählen", "지우개 · E · 프리셋 선택", "橡皮擦 · E · 选择预设", "消しゴム · E · プリセットを選択"]),
    ("Фигуры · L / R / O · нажмите для выбора", ["Shapes · L / R / O · choose a shape", "Formen · L / R / O · Form wählen", "도형 · L / R / O · 도형 선택", "形状 · L / R / O · 选择形状", "図形 · L / R / O · 図形を選択"]),
    ("Параметры кисти · F4", ["Brush settings · F4", "Pinseleinstellungen · F4", "브러시 설정 · F4", "画笔设置 · F4", "ブラシ設定 · F4"]),
    ("Параметры ластика · F4", ["Eraser settings · F4", "Radiereinstellungen · F4", "지우개 설정 · F4", "橡皮擦设置 · F4", "消しゴム設定 · F4"]),
    ("Параметры фигуры · F4", ["Shape settings · F4", "Formeinstellungen · F4", "도형 설정 · F4", "形状设置 · F4", "図形設定 · F4"]),
    ("Непрозрачность заливки · F4", ["Fill opacity · F4", "Deckkraft der Füllung · F4", "채우기 불투명도 · F4", "填充不透明度 · F4", "塗りつぶしの不透明度 · F4"]),
    ("Выберите кисть, ластик, фигуру или заливку", ["Choose a brush, eraser, shape or fill", "Pinsel, Radierer, Form oder Füllung wählen", "브러시, 지우개, 도형 또는 채우기를 선택하세요", "请选择画笔、橡皮擦、形状或填充", "ブラシ、消しゴム、図形、塗りつぶしを選択"]),
    ("Зеркальная кисть · M", ["Mirror painting · M", "Spiegeln · M", "대칭 그리기 · M", "镜像绘画 · M", "対称描画 · M"]),
    ("Просмотр тона · V", ["Grayscale preview · V", "Graustufenvorschau · V", "흑백 미리보기 · V", "灰度预览 · V", "グレースケール表示 · V"]),
    ("Сетка · Ctrl+'", ["Grid · Ctrl+'", "Raster · Ctrl+'", "격자 · Ctrl+'", "网格 · Ctrl+'", "グリッド · Ctrl+'"]),
    ("Уменьшить масштаб · −", ["Zoom out · −", "Verkleinern · −", "축소 · −", "缩小 · −", "縮小 · −"]),
    ("Масштаб 100% · 1", ["Zoom 100% · 1", "Zoom 100% · 1", "100% 배율 · 1", "100% 缩放 · 1", "倍率100% · 1"]),
    ("Увеличить масштаб · +", ["Zoom in · +", "Vergrößern · +", "확대 · +", "放大 · +", "拡大 · +"]),
    ("Вписать холст · 0", ["Fit canvas · 0", "Leinwand einpassen · 0", "캔버스 맞춤 · 0", "适合画布 · 0", "キャンバスを合わせる · 0"]),
    ("Выбор инструмента", ["Tool selection", "Werkzeugauswahl", "도구 선택", "工具选择", "ツール選択"]),
    ("Поиск…", ["Search…", "Suchen…", "검색…", "搜索…", "検索…"]),
    ("Поиск кистей", ["Search brushes", "Pinsel suchen", "브러시 검색", "搜索画笔", "ブラシを検索"]),
    ("Поиск фигур", ["Search shapes", "Formen suchen", "도형 검색", "搜索形状", "図形を検索"]),
    ("Поиск настроек", ["Search settings", "Einstellungen suchen", "설정 검색", "搜索设置", "設定を検索"]),
    ("Поиск настроек…", ["Search settings…", "Einstellungen suchen…", "설정 검색…", "搜索设置…", "設定を検索…"]),
    ("Прокрутка настроек", ["Scroll settings", "Einstellungen scrollen", "설정 스크롤", "滚动设置", "設定をスクロール"]),
    ("Измените запрос или очистите поиск", ["Try another query or clear the search", "Suchbegriff ändern oder Suche löschen", "검색어를 바꾸거나 검색을 지우세요", "更改关键词或清除搜索", "検索語を変更するか検索をクリアしてください"]),
    ("Очистить поиск", ["Clear search", "Suche löschen", "검색 지우기", "清除搜索", "検索をクリア"]),
    ("Закрыть поиск", ["Close search", "Suche schließen", "검색 닫기", "关闭搜索", "検索を閉じる"]),
    ("Ничего не найдено", ["No results", "Keine Ergebnisse", "검색 결과 없음", "未找到结果", "見つかりません"]),
    ("Категории кистей", ["Brush categories", "Pinselkategorien", "브러시 카테고리", "画笔分类", "ブラシのカテゴリ"]),
    ("Категории фигур", ["Shape categories", "Formkategorien", "도형 카테고리", "形状分类", "図形のカテゴリ"]),
    ("Все", ["All", "Alle", "전체", "全部", "すべて"]),
    ("Скетч", ["Sketch", "Skizze", "스케치", "素描", "スケッチ"]),
    ("Живопись", ["Painting", "Malerei", "페인팅", "绘画", "ペイント"]),
    ("Мягкие", ["Soft", "Weich", "부드러움", "柔和", "ソフト"]),
    ("Фактура", ["Texture", "Textur", "질감", "纹理", "テクスチャ"]),
    ("Распыление", ["Spray", "Spray", "분사", "喷涂", "スプレー"]),
    ("Плоские", ["Flat", "Flach", "평붓", "平头", "平筆"]),
    ("Линии", ["Lines", "Linien", "선", "直线", "直線"]),
    ("Углы", ["Angular", "Eckig", "각진 모양", "棱角形状", "角形"]),
    ("Кривые", ["Curves", "Kurven", "곡선", "曲线", "曲線"]),
    ("Сначала завершите текущую правку.", ["Finish the current edit first.", "Beenden Sie zuerst die aktuelle Bearbeitung.", "먼저 현재 편집을 완료하세요.", "请先完成当前编辑。", "先に現在の編集を完了してください。"]),
    ("КИСТЬ", ["BRUSH", "PINSEL", "브러시", "画笔", "ブラシ"]),
    ("ЛАСТИК", ["ERASER", "RADIERER", "지우개", "橡皮擦", "消しゴム"]),
    ("ФИГУРЫ", ["SHAPES", "FORMEN", "도형", "形状", "図形"]),
    ("Фигуры", ["Shapes", "Formen", "도형", "形状", "図形"]),
    ("Закрыть выбор · Esc", ["Close selection · Esc", "Auswahl schließen · Esc", "선택 닫기 · Esc", "关闭选择 · Esc", "選択を閉じる · Esc"]),
    ("Закрепить панель", ["Pin panel", "Bedienfeld anheften", "패널 고정", "固定面板", "パネルを固定"]),
    ("Открепить панель", ["Unpin panel", "Bedienfeld lösen", "패널 고정 해제", "取消固定面板", "パネルの固定を解除"]),
    ("Переместить панель", ["Move panel", "Bedienfeld verschieben", "패널 이동", "移动面板", "パネルを移動"]),
    ("Переместить панель выбора", ["Move selection panel", "Auswahlfeld verschieben", "선택 패널 이동", "移动选择面板", "選択パネルを移動"]),
    ("Переместить панель · стрелки / Home", ["Move panel · arrows / Home", "Bedienfeld verschieben · Pfeile / Pos1", "패널 이동 · 방향키 / Home", "移动面板 · 方向键 / Home", "パネルを移動 · 矢印キー / Home"]),
    ("Перетащить панель · щелчок — открепить", ["Drag panel · click to unpin", "Bedienfeld ziehen · Klick zum Lösen", "패널 끌기 · 클릭하여 고정 해제", "拖动面板 · 单击取消固定", "パネルをドラッグ · クリックで固定解除"]),
    ("Панели возвращены на место", ["Panel positions reset", "Panelpositionen zurückgesetzt", "패널 위치가 초기화되었습니다", "面板位置已重置", "パネルの位置をリセットしました"]),
    ("Вернуть панель на место", ["Reset panel position", "Panelposition zurücksetzen", "패널 위치 초기화", "重置面板位置", "パネルの位置をリセット"]),
    ("Сбросить ластик", ["Reset eraser", "Radierer zurücksetzen", "지우개 초기화", "重置橡皮擦", "消しゴムをリセット"]),
    ("Поиск ластиков", ["Search erasers", "Radierer suchen", "지우개 검색", "搜索橡皮擦", "消しゴムを検索"]),
    ("Тушь", ["Ink", "Tusche", "잉크", "墨水", "インク"]),
    ("Мягкая", ["Soft", "Weich", "부드러운 브러시", "柔边", "ソフト"]),
    ("Маркер", ["Marker", "Marker", "마커", "马克笔", "マーカー"]),
    ("Карандаш", ["Pencil", "Bleistift", "연필", "铅笔", "鉛筆"]),
    ("Уголь", ["Charcoal", "Kohle", "목탄", "炭笔", "木炭"]),
    ("Мел", ["Chalk", "Kreide", "분필", "粉笔", "チョーク"]),
    ("Аэрограф", ["Airbrush", "Airbrush", "에어브러시", "喷枪", "エアブラシ"]),
    ("Каллиграфия", ["Calligraphy", "Kalligrafie", "캘리그래피", "书法", "カリグラフィー"]),
    ("Брызги", ["Spray", "Spritzer", "스프레이", "喷溅", "スプレー"]),
    ("Плоская", ["Flat", "Flach", "평붓", "平头", "平筆"]),
    ("Чёткий", ["Hard", "Hart", "단단함", "硬边", "ハード"]),
    ("Мягкий", ["Soft", "Weich", "부드러움", "柔边", "ソフト"]),
    ("Плоский", ["Flat", "Flach", "평평함", "平头", "フラット"]),
    ("Твёрдый", ["Hard", "Hart", "단단함", "硬边", "ハード"]),
    ("Точный", ["Precise", "Präzise", "정밀", "精细", "精密"]),
    ("Фактурный", ["Textured", "Strukturiert", "질감", "纹理", "テクスチャ"]),
    ("Зернистый", ["Grainy", "Körnig", "입자", "颗粒", "粒状"]),
    ("Воздушный", ["Airy", "Luftig", "에어", "轻柔", "エア"]),
    ("Скошенный", ["Angled", "Abgeschrägt", "사선", "斜角", "斜め"]),
    ("Крапчатый", ["Speckled", "Gesprenkelt", "반점", "斑点", "斑点"]),
    ("Широкий", ["Wide", "Breit", "넓음", "宽幅", "ワイド"]),
    ("Предыдущая страница", ["Previous page", "Vorherige Seite", "이전 페이지", "上一页", "前のページ"]),
    ("Следующая страница", ["Next page", "Nächste Seite", "다음 페이지", "下一页", "次のページ"]),
    ("Цвет и слои", ["Color and layers", "Farbe und Ebenen", "색상 및 레이어", "颜色与图层", "色とレイヤー"]),
    ("Цвет", ["Color", "Farbe", "색상", "颜色", "カラー"]),
    ("Слои", ["Layers", "Ebenen", "레이어", "图层", "レイヤー"]),
    ("ЦВЕТ", ["COLOR", "FARBE", "색상", "颜色", "カラー"]),
    ("HEX · точный цвет", ["HEX · exact color", "HEX · genaue Farbe", "HEX · 정확한 색상", "HEX · 精确颜色", "HEX · 正確な色"]),
    ("Цвет: стрелки меняют насыщенность и яркость", ["Color: arrows adjust saturation and brightness", "Farbe: Pfeile ändern Sättigung und Helligkeit", "색상: 방향키로 채도와 밝기 조절", "颜色：方向键调整饱和度和亮度", "色：矢印キーで彩度と明度を調整"]),
    ("Оттенок: стрелки меняют значение", ["Hue: arrows adjust the value", "Farbton: Pfeile ändern den Wert", "색조: 방향키로 값 조절", "色相：方向键调整数值", "色相：矢印キーで値を調整"]),
    ("НЕДАВНИЕ", ["RECENT", "ZULETZT", "최근 색상", "最近使用", "最近の色"]),
    ("Добавить слой · Ctrl+Shift+N", ["Add layer · Ctrl+Shift+N", "Ebene hinzufügen · Ctrl+Shift+N", "레이어 추가 · Ctrl+Shift+N", "添加图层 · Ctrl+Shift+N", "レイヤーを追加 · Ctrl+Shift+N"]),
    ("Непрозрачность слоя", ["Layer opacity", "Ebenendeckkraft", "레이어 불투명도", "图层不透明度", "レイヤーの不透明度"]),
    ("Показать / скрыть слой", ["Show / hide layer", "Ebene ein-/ausblenden", "레이어 표시/숨기기", "显示/隐藏图层", "レイヤーを表示／非表示"]),
    ("Заблокировать / разблокировать слой", ["Lock / unlock layer", "Ebene sperren/entsperren", "레이어 잠금/해제", "锁定/解锁图层", "レイヤーをロック／解除"]),
    ("Имя слоя · F2", ["Layer name · F2", "Ebenenname · F2", "레이어 이름 · F2", "图层名称 · F2", "レイヤー名 · F2"]),
    ("Дублировать слой", ["Duplicate layer", "Ebene duplizieren", "레이어 복제", "复制图层", "レイヤーを複製"]),
    ("Переместить слой выше", ["Move layer up", "Ebene nach oben", "레이어 위로 이동", "上移图层", "レイヤーを上へ"]),
    ("Переместить слой ниже", ["Move layer down", "Ebene nach unten", "레이어 아래로 이동", "下移图层", "レイヤーを下へ"]),
    ("Удалить слой · Ctrl+Delete", ["Delete layer · Ctrl+Delete", "Ebene löschen · Ctrl+Delete", "레이어 삭제 · Ctrl+Delete", "删除图层 · Ctrl+Delete", "レイヤーを削除 · Ctrl+Delete"]),
    ("Параметры инструмента", ["Tool settings", "Werkzeugeinstellungen", "도구 설정", "工具设置", "ツール設定"]),
    ("Закрыть параметры · F4 / Esc", ["Close settings · F4 / Esc", "Einstellungen schließen · F4 / Esc", "설정 닫기 · F4 / Esc", "关闭设置 · F4 / Esc", "設定を閉じる · F4 / Esc"]),
    ("Толщина", ["Width", "Breite", "두께", "线宽", "太さ"]),
    ("Размер", ["Size", "Größe", "크기", "大小", "サイズ"]),
    ("Сила стирания", ["Erase strength", "Radierstärke", "지우기 강도", "擦除强度", "消去の強さ"]),
    ("Непрозрачность", ["Opacity", "Deckkraft", "불투명도", "不透明度", "不透明度"]),
    ("Основные", ["Basic", "Basis", "기본", "基本", "基本"]),
    ("Отпечаток", ["Tip", "Spitze", "브러시 팁", "笔尖", "ブラシ先端"]),
    ("Образец", ["Preview", "Vorschau", "미리 보기", "预览", "プレビュー"]),
    ("Заливка фигуры", ["Shape fill", "Formfüllung", "도형 채우기", "形状填充", "図形の塗りつぶし"]),
    ("Прокрутка параметров", ["Scroll tool settings", "Werkzeugeinstellungen scrollen", "도구 설정 스크롤", "滚动工具设置", "ツール設定をスクロール"]),
    ("Прокрутить параметры", ["Scroll tool settings", "Werkzeugeinstellungen scrollen", "도구 설정 스크롤", "滚动工具设置", "ツール設定をスクロール"]),
    ("Заливка доступна для замкнутых фигур", ["Fill is available for closed shapes", "Füllung ist bei geschlossenen Formen verfügbar", "닫힌 도형만 채울 수 있습니다", "仅闭合形状支持填充", "閉じた図形を塗りつぶせます"]),
    ("Контур и заливка", ["Outline and fill", "Kontur und Füllung", "윤곽선 및 채우기", "轮廓和填充", "輪郭と塗りつぶし"]),
    ("Число — точный ввод", ["Number: exact input", "Zahl: genaue Eingabe", "숫자: 직접 입력", "数值：精确输入", "数値：直接入力"]),
    ("Жёсткость", ["Hardness", "Härte", "경도", "硬度", "硬さ"]),
    ("Подача краски", ["Flow", "Farbfluss", "물감 흐름", "流量", "流量"]),
    ("Интервал", ["Spacing", "Abstand", "간격", "间距", "間隔"]),
    ("Угол", ["Angle", "Winkel", "각도", "角度", "角度"]),
    ("Разброс", ["Scatter", "Streuung", "흩뿌리기", "散布", "散布"]),
    ("Сбросить кисть", ["Reset brush", "Pinsel zurücksetzen", "브러시 초기화", "重置画笔", "ブラシをリセット"]),
    ("Число — точный ввод · F4 / Esc — закрыть", ["Number: exact input · F4 / Esc: close", "Zahl: exakte Eingabe · F4 / Esc: schließen", "숫자: 직접 입력 · F4 / Esc: 닫기", "数值：精确输入 · F4 / Esc：关闭", "数値：直接入力 · F4 / Esc：閉じる"]),
    ("Настройки внешнего вида", ["Appearance settings", "Darstellung", "화면 설정", "外观设置", "外観設定"]),
    ("Настройки", ["Settings", "Einstellungen", "설정", "设置", "設定"]),
    ("Ваше пространство для рисунка", ["Your space to create", "Ihr Raum zum Zeichnen", "나만의 그림 공간", "你的绘画空间", "あなたの創作空間"]),
    ("Закрыть настройки · Esc", ["Close settings · Esc", "Einstellungen schließen · Esc", "설정 닫기 · Esc", "关闭设置 · Esc", "設定を閉じる · Esc"]),
    ("Вид", ["Appearance", "Darstellung", "화면", "外观", "外観"]),
    ("Текст", ["Text", "Text", "텍스트", "文字", "テキスト"]),
    ("Язык", ["Language", "Sprache", "언어", "语言", "言語"]),
    ("Язык интерфейса", ["Interface language", "Oberflächensprache", "인터페이스 언어", "界面语言", "表示言語"]),
    ("ТЕМА", ["THEME", "DESIGN", "테마", "主题", "テーマ"]),
    ("Графит", ["Graphite", "Graphit", "흑연", "石墨", "グラファイト"]),
    ("Бумага", ["Paper", "Papier", "종이", "纸张", "ペーパー"]),
    ("ШРИФТ ИНТЕРФЕЙСА", ["INTERFACE FONT", "OBERFLÄCHENSCHRIFT", "인터페이스 글꼴", "界面字体", "表示フォント"]),
    ("Шрифт интерфейса", ["Interface font", "Oberflächenschrift", "인터페이스 글꼴", "界面字体", "表示フォント"]),
    ("Аа Бб · 0123", ["Aa Bb · 0123", "Aa Bb · 0123", "가 나 · 0123", "画笔 · 0123", "あ ア · 0123"]),
    ("Шрифт применяется ко всему интерфейсу.", ["The font applies to the whole interface.", "Die Schrift gilt für die gesamte Oberfläche.", "글꼴이 전체 인터페이스에 적용됩니다.", "字体应用于整个界面。", "フォントは画面全体に適用されます。"]),
    ("АКЦЕНТ", ["ACCENT", "AKZENTFARBE", "강조색", "强调色", "アクセント"]),
    ("Сбросить вид", ["Reset appearance", "Zurücksetzen", "화면 초기화", "重置外观", "外観をリセット"]),
    ("Сохраняется автоматически", ["Saved automatically", "Wird automatisch gespeichert", "자동으로 저장됩니다", "自动保存", "自動保存されます"]),
    ("Размер текста", ["Text size", "Textgröße", "텍스트 크기", "文字大小", "文字サイズ"]),
    ("Анимации панелей", ["Panel animations", "Panelanimationen", "패널 애니메이션", "面板动画", "パネルのアニメーション"]),
    ("Плавно", ["Smooth", "Sanft", "부드럽게", "平滑", "スムーズ"]),
    ("Без анимаций", ["No animations", "Ohne Animation", "애니메이션 없음", "无动画", "アニメーションなし"]),
    ("Учитываются настройки Windows", ["Respects Windows settings", "Beachtet Windows-Einstellungen", "Windows 설정을 따릅니다", "遵循 Windows 设置", "Windowsの設定に従います"]),
    ("Образец текста · Portfolio 0123", ["Text sample · Portfolio 0123", "Schriftprobe · Portfolio 0123", "텍스트 예제 · Portfolio 0123", "文字示例 · Portfolio 0123", "文字サンプル · Portfolio 0123"]),
    ("Шрифты MIT", ["MIT fonts", "MIT-Schriften", "MIT 글꼴", "MIT 字体", "MITフォント"]),
    ("Системные шрифты", ["System fonts", "Systemschriften", "시스템 글꼴", "系统字体", "システムフォント"]),
    ("Новый холст", ["New canvas", "Neue Leinwand", "새 캔버스", "新建画布", "新しいキャンバス"]),
    ("Открыть проект…", ["Open project…", "Projekt öffnen…", "프로젝트 열기…", "打开项目…", "プロジェクトを開く…"]),
    ("Добавить изображение…", ["Add image…", "Bild hinzufügen…", "이미지 추가…", "添加图像…", "画像を追加…"]),
    ("Сохранить", ["Save", "Speichern", "저장", "保存", "保存"]),
    ("Сохранить как…", ["Save as…", "Speichern unter…", "다른 이름으로 저장…", "另存为…", "名前を付けて保存…"]),
    ("Экспорт PNG…", ["Export PNG…", "PNG exportieren…", "PNG 내보내기…", "导出 PNG…", "PNGを書き出す…"]),
    ("Открыть этюд FORM 01", ["Open FORM 01 study", "Studie FORM 01 öffnen", "FORM 01 습작 열기", "打开 FORM 01 习作", "FORM 01習作を開く"]),
    ("Клавиши и инструменты", ["Keys and tools", "Tasten und Werkzeuge", "단축키 및 도구", "快捷键与工具", "キーとツール"]),
    ("Закрыть · Escape", ["Close · Escape", "Schließen · Escape", "닫기 · Escape", "关闭 · Escape", "閉じる · Escape"]),
    ("Место для новой идеи.", ["Room for a new idea.", "Platz für eine neue Idee.", "새로운 아이디어를 위한 공간.", "为新想法留出空间。", "新しいアイデアのための場所。"]),
    ("Выберите формат. Остальное — ваш почерк.", ["Choose a format. Make it your own.", "Format wählen. Den Rest gestalten Sie.", "크기를 고르고 나만의 작품을 그려보세요.", "选择画布尺寸，创作自己的风格。", "サイズを選び、自分らしく描きましょう。"]),
    ("Этюд", ["Study", "Studie", "습작", "习作", "習作"]),
    ("Квадрат", ["Square", "Quadrat", "정사각형", "正方形", "正方形"]),
    ("Постер", ["Poster", "Poster", "포스터", "海报", "ポスター"]),
    ("Ширина / px", ["Width / px", "Breite / px", "너비 / px", "宽度 / px", "幅 / px"]),
    ("Высота / px", ["Height / px", "Höhe / px", "높이 / px", "高度 / px", "高さ / px"]),
    ("Бумага + пустой слой · до 4096 px на сторону", ["Paper + empty layer · up to 4096 px per side", "Papier + leere Ebene · bis 4096 px je Seite", "종이 + 빈 레이어 · 한 변 최대 4096 px", "纸张和空白图层 · 每边最多 4096 px", "用紙＋空のレイヤー · 各辺最大4096 px"]),
    ("Создать холст", ["Create canvas", "Leinwand erstellen", "캔버스 만들기", "创建画布", "キャンバスを作成"]),
    ("Рисуйте в своём ритме.", ["Draw at your own pace.", "Zeichnen Sie in Ihrem Tempo.", "나만의 속도로 그려보세요.", "按自己的节奏绘画。", "自分のペースで描きましょう。"]),
    ("Portfolio / Краткая карта студии", ["Portfolio / Studio shortcuts", "Portfolio / Studio-Kurzbefehle", "Portfolio / 스튜디오 단축키", "Portfolio / 工作室快捷键", "Portfolio / ショートカット"]),
    ("Кисть / ластик / заливка / пипетка", ["Brush / eraser / fill / eyedropper", "Pinsel / Radierer / Füllen / Pipette", "브러시 / 지우개 / 채우기 / 스포이트", "画笔 / 橡皮擦 / 填充 / 吸管", "ブラシ／消しゴム／塗りつぶし／スポイト"]),
    ("Линия / прямоугольник / эллипс", ["Line / rectangle / ellipse", "Linie / Rechteck / Ellipse", "선 / 사각형 / 타원", "直线 / 矩形 / 椭圆", "直線／長方形／楕円"]),
    ("Пробел + движение", ["Space + drag", "Leertaste + Ziehen", "스페이스 + 드래그", "空格键 + 拖动", "スペース＋ドラッグ"]),
    ("Перемещение холста", ["Pan canvas", "Leinwand verschieben", "캔버스 이동", "平移画布", "キャンバスを移動"]),
    ("Колесо / 0 / 1", ["Wheel / 0 / 1", "Mausrad / 0 / 1", "휠 / 0 / 1", "滚轮 / 0 / 1", "ホイール／0／1"]),
    ("Масштаб / вписать / 100%", ["Zoom / fit / 100%", "Zoom / einpassen / 100%", "확대·축소 / 맞춤 / 100%", "缩放 / 适合画布 / 100%", "ズーム／全体表示／100%"]),
    ("Уменьшить / увеличить кисть", ["Decrease / increase brush size", "Pinsel verkleinern / vergrößern", "브러시 축소 / 확대", "减小 / 增大画笔", "ブラシを小さく／大きく"]),
    ("Зеркало / тон / режим фокуса", ["Mirror / grayscale / hide panels", "Spiegeln / Graustufen / Panels ausblenden", "대칭 / 흑백 / 패널 숨기기", "镜像 / 灰度 / 隐藏面板", "対称／グレースケール／パネル非表示"]),
    ("Отмена / повтор", ["Undo / redo", "Rückgängig / wiederholen", "실행 취소 / 다시 실행", "撤销 / 重做", "元に戻す／やり直す"]),
    ("Сохранить проект / PNG", ["Save project / PNG", "Projekt / PNG speichern", "프로젝트 / PNG 저장", "保存项目 / PNG", "プロジェクト／PNGを保存"]),
    ("Фокус по элементам интерфейса", ["Move focus between controls", "Fokus zwischen Bedienelementen", "컨트롤 간 포커스 이동", "在控件间移动焦点", "コントロール間のフォーカス移動"]),
    ("Enter / стрелки", ["Enter / arrows", "Enter / Pfeile", "Enter / 방향키", "Enter / 方向键", "Enter／矢印キー"]),
    ("Активировать / точное изменение", ["Activate / precise adjustment", "Aktivieren / genau einstellen", "활성화 / 정밀 조절", "激活 / 精确调整", "実行／正確な調整"]),
    ("Отменить жест / закрыть панель", ["Cancel gesture / close panel", "Geste abbrechen / Panel schließen", "동작 취소 / 패널 닫기", "取消操作 / 关闭面板", "操作を取り消す／パネルを閉じる"]),
    ("F2 — имя слоя. Alt — временная пипетка. Цвета рисунка не зависят от темы.", ["F2: layer name. Alt: temporary eyedropper. Themes do not change artwork colors.", "F2: Ebenenname. Alt: temporäre Pipette. Das Design ändert keine Bildfarben.", "F2: 레이어 이름. Alt: 임시 스포이트. 테마는 그림의 색상을 바꾸지 않습니다.", "F2：图层名称。Alt：临时吸管。主题不会改变画作颜色。", "F2：レイヤー名。Alt：一時的なスポイト。テーマは作品の色を変えません。"]),
    ("Дождитесь окончания записи.", ["Please wait for the write to finish.", "Warten Sie, bis der Schreibvorgang endet.", "저장이 끝날 때까지 기다려 주세요.", "请等待写入完成。", "書き込みが完了するまでお待ちください。"]),
    ("Действие отменено", ["Action undone", "Aktion rückgängig gemacht", "실행을 취소했습니다", "已撤销操作", "操作を元に戻しました"]),
    ("Действие повторено", ["Action redone", "Aktion wiederholt", "다시 실행했습니다", "已重做操作", "操作をやり直しました"]),
    ("Tab — вернуть / скрыть панели", ["Tab: show / hide panels", "Tab: Panels ein-/ausblenden", "Tab: 패널 표시/숨기기", "Tab：显示/隐藏面板", "Tab：パネルを表示／非表示"]),
    ("Зеркало: штрих отражается по центру холста", ["Mirror: strokes reflect across the canvas center", "Spiegeln: Striche werden an der Leinwandmitte gespiegelt", "대칭: 획이 캔버스 중앙을 기준으로 반사됩니다", "镜像：笔画沿画布中心对称", "対称：キャンバス中央でストロークを反転"]),
    ("Зеркало выключено", ["Mirror off", "Spiegeln aus", "대칭 해제", "镜像已关闭", "対称描画をオフ"]),
    ("Просмотр тона. Исходные цвета и PNG остаются цветными.", ["Grayscale preview. Original artwork and PNG retain their colors.", "Graustufenvorschau. Original und PNG behalten ihre Farben.", "흑백 미리보기입니다. 원본과 PNG의 색상은 유지됩니다.", "灰度预览。原作和 PNG 仍保留颜色。", "グレースケール表示です。原画とPNGの色は保持されます。"]),
    ("Цветной просмотр", ["Color preview", "Farbvorschau", "컬러 미리보기", "彩色预览", "カラー表示"]),
    ("Размер ластика", ["Eraser size", "Radierergröße", "지우개 크기", "橡皮擦大小", "消しゴムのサイズ"]),
    ("Толщина контура", ["Outline width", "Konturbreite", "윤곽선 두께", "轮廓线宽", "輪郭の太さ"]),
    ("Размер кисти", ["Brush size", "Pinselgröße", "브러시 크기", "画笔大小", "ブラシサイズ"]),
    ("Размер в пикселях: от 1 до 200", ["Size in pixels: 1 to 200", "Größe in Pixeln: 1 bis 200", "크기: 1~200픽셀", "像素大小：1 至 200", "ピクセルサイズ：1～200"]),
    ("Размер в пикселях: от 1 до 512", ["Size in pixels: 1 to 512", "Größe in Pixeln: 1 bis 512", "크기: 1~512픽셀", "像素大小：1 至 512", "ピクセルサイズ：1～512"]),
    ("Сила ластика", ["Eraser strength", "Radierstärke", "지우개 강도", "橡皮擦强度", "消しゴムの強さ"]),
    ("Проценты: от 0 до 100", ["Percentage: 0 to 100", "Prozent: 0 bis 100", "백분율: 0~100", "百分比：0 至 100", "割合：0～100"]),
    ("Точный цвет", ["Exact color", "Genaue Farbe", "정확한 색상", "精确颜色", "正確な色"]),
    ("Шесть символов HEX, например #365AD7", ["Six HEX digits, for example #365AD7", "Sechs HEX-Zeichen, z. B. #365AD7", "HEX 6자리, 예: #365AD7", "六位 HEX 代码，例如 #365AD7", "6桁のHEX値（例：#365AD7）"]),
    ("Стрелки: насыщенность и яркость. HEX — точный цвет.", ["Arrows: saturation and brightness. HEX: exact color.", "Pfeile: Sättigung und Helligkeit. HEX: genaue Farbe.", "방향키: 채도와 밝기. HEX: 정확한 색상.", "方向键：饱和度和亮度。HEX：精确颜色。", "矢印キー：彩度と明度。HEX：正確な色。"]),
    ("Стрелки: оттенок. HEX — точный цвет.", ["Arrows: hue. HEX: exact color.", "Pfeile: Farbton. HEX: genaue Farbe.", "방향키: 색조. HEX: 정확한 색상.", "方向键：色相。HEX：精确颜色。", "矢印キー：色相。HEX：正確な色。"]),
    ("Имя слоя", ["Layer name", "Ebenenname", "레이어 이름", "图层名称", "レイヤー名"]),
    ("Название слоя", ["Layer name", "Ebenenname", "레이어 이름", "图层名称", "レイヤー名"]),
    ("Слой удалён. Ctrl+Z — вернуть.", ["Layer deleted. Ctrl+Z to restore.", "Ebene gelöscht. Ctrl+Z zum Wiederherstellen.", "레이어를 삭제했습니다. Ctrl+Z로 복원하세요.", "图层已删除。按 Ctrl+Z 恢复。", "レイヤーを削除しました。Ctrl+Zで復元。"]),
    ("Ширина холста", ["Canvas width", "Leinwandbreite", "캔버스 너비", "画布宽度", "キャンバスの幅"]),
    ("Пиксели: от 1 до 4096", ["Pixels: 1 to 4096", "Pixel: 1 bis 4096", "픽셀: 1~4096", "像素：1 至 4096", "ピクセル：1～4096"]),
    ("Высота холста", ["Canvas height", "Leinwandhöhe", "캔버스 높이", "画布高度", "キャンバスの高さ"]),
    ("Новый этюд", ["New study", "Neue Studie", "새 습작", "新习作", "新しい習作"]),
    ("Новый холст готов.", ["New canvas ready.", "Neue Leinwand bereit.", "새 캔버스가 준비되었습니다.", "新画布已就绪。", "新しいキャンバスを作成しました。"]),
    ("Введите HEX в формате #RRGGBB. Например: #365AD7", ["Enter HEX as #RRGGBB. Example: #365AD7", "HEX im Format #RRGGBB eingeben, z. B. #365AD7", "#RRGGBB 형식의 HEX를 입력하세요. 예: #365AD7", "请输入 #RRGGBB 格式的 HEX，例如 #365AD7", "#RRGGBB形式で入力してください（例：#365AD7）"]),
    ("Размер стороны должен быть от 1 до 4096 пикселей.", ["Each side must be 1 to 4096 pixels.", "Jede Seite muss 1 bis 4096 Pixel lang sein.", "각 변은 1~4096픽셀이어야 합니다.", "每边必须为 1 至 4096 像素。", "各辺は1～4096ピクセルにしてください。"]),
    ("В этой точке прозрачный пиксель.", ["This pixel is transparent.", "Dieses Pixel ist transparent.", "이 픽셀은 투명합니다.", "此像素是透明的。", "このピクセルは透明です。"]),
    ("Слой скрыт или заблокирован. Выберите доступный слой.", ["The layer is hidden or locked. Choose an editable layer.", "Die Ebene ist verborgen oder gesperrt. Wählen Sie eine bearbeitbare Ebene.", "레이어가 숨겨져 있거나 잠겨 있습니다. 편집 가능한 레이어를 선택하세요.", "图层已隐藏或锁定。请选择可编辑图层。", "レイヤーが非表示またはロック中です。編集可能なレイヤーを選んでください。"]),
    ("Проект открыт", ["Project opened", "Projekt geöffnet", "프로젝트를 열었습니다", "项目已打开", "プロジェクトを開きました"]),
    ("Изображение", ["Image", "Bild", "이미지", "图像", "画像"]),
    ("Изображение добавлено отдельным слоем", ["Image added as a separate layer", "Bild als separate Ebene hinzugefügt", "이미지를 별도 레이어로 추가했습니다", "图像已添加为独立图层", "画像を別のレイヤーに追加しました"]),
    ("Этюд восстановлен. Сохраните его в нужную папку.", ["Study recovered. Save it to your preferred folder.", "Studie wiederhergestellt. Speichern Sie sie im gewünschten Ordner.", "습작을 복원했습니다. 원하는 폴더에 저장하세요.", "习作已恢复。请将其保存到所需文件夹。", "習作を復元しました。保存先を選んで保存してください。"]),
    ("Предыдущая резервная копия сохранена до следующего запуска.", ["The previous recovery copy is kept until the next launch.", "Die vorige Sicherung bleibt bis zum nächsten Start erhalten.", "이전 복구 사본은 다음 실행까지 보관됩니다.", "上次的备份将保留到下次启动。", "以前のバックアップは次回起動まで保持されます。"]),
    ("Ваш первый этюд. Рисуйте поверх или создайте чистый холст.", ["Your first study. Draw over it or create a blank canvas.", "Ihre erste Studie. Zeichnen Sie darauf oder erstellen Sie eine leere Leinwand.", "첫 습작입니다. 위에 그리거나 빈 캔버스를 만드세요.", "你的第一幅习作。可以继续绘画或创建空白画布。", "最初の習作です。上から描くか、空のキャンバスを作成してください。"]),
    ("Дождитесь завершения сохранения.", ["Please wait for saving to finish.", "Warten Sie, bis das Speichern abgeschlossen ist.", "저장이 완료될 때까지 기다려 주세요.", "请等待保存完成。", "保存が完了するまでお待ちください。"]),
    ("Сохранить изменения?", ["Save changes?", "Änderungen speichern?", "변경 사항을 저장할까요?", "保存更改？", "変更を保存しますか？"]),
    ("Сохранить рисунок перед продолжением?\n\nДа — сохранить. Нет — продолжить без сохранения. Отмена — вернуться к рисунку.", ["Save your artwork before continuing?\n\nYes: save. No: continue without saving. Cancel: return to the artwork.", "Bild vor dem Fortfahren speichern?\n\nJa: speichern. Nein: ohne Speichern fortfahren. Abbrechen: zum Bild zurückkehren.", "계속하기 전에 그림을 저장할까요?\n\n예: 저장. 아니요: 저장하지 않고 계속. 취소: 그림으로 돌아가기.", "继续前保存画作吗？\n\n是：保存。否：不保存并继续。取消：返回画作。", "続行する前に作品を保存しますか？\n\nはい：保存。いいえ：保存せずに続行。キャンセル：作品に戻る。"]),
    ("Этюд открыт. Все элементы доступны на слоях.", ["Study opened. All elements are available as layers.", "Studie geöffnet. Alle Elemente sind als Ebenen verfügbar.", "습작을 열었습니다. 모든 요소는 레이어에 있습니다.", "习作已打开。所有元素均可在图层中编辑。", "習作を開きました。各要素はレイヤーで編集できます。"]),
    ("Сохраняем проект…", ["Saving project…", "Projekt wird gespeichert…", "프로젝트 저장 중…", "正在保存项目…", "プロジェクトを保存中…"]),
    ("Экспортируем PNG…", ["Exporting PNG…", "PNG wird exportiert…", "PNG 내보내는 중…", "正在导出 PNG…", "PNGを書き出し中…"]),
    ("Открываем проект…", ["Opening project…", "Projekt wird geöffnet…", "프로젝트 여는 중…", "正在打开项目…", "プロジェクトを開いています…"]),
    ("Восстанавливаем проект…", ["Recovering project…", "Projekt wird wiederhergestellt…", "프로젝트 복원 중…", "正在恢复项目…", "プロジェクトを復元中…"]),
    ("Загружаем изображение…", ["Loading image…", "Bild wird geladen…", "이미지 불러오는 중…", "正在加载图像…", "画像を読み込み中…"]),
    ("Найден незавершённый этюд", ["Unfinished study found", "Unfertige Studie gefunden", "미완성 습작을 찾았습니다", "发现未完成的习作", "未完了の習作が見つかりました"]),
    ("Восстановить рисунок из резервной копии?\n\nДа — восстановить. Нет — удалить резервную копию. Отмена — оставить копию до следующего запуска.", ["Recover artwork from the backup?\n\nYes: recover. No: delete the backup. Cancel: keep it until the next launch.", "Bild aus der Sicherung wiederherstellen?\n\nJa: wiederherstellen. Nein: Sicherung löschen. Abbrechen: bis zum nächsten Start behalten.", "백업에서 그림을 복원할까요?\n\n예: 복원. 아니요: 백업 삭제. 취소: 다음 실행까지 보관.", "从备份恢复画作吗？\n\n是：恢复。否：删除备份。取消：保留到下次启动。", "バックアップから作品を復元しますか？\n\nはい：復元。いいえ：バックアップを削除。キャンセル：次回起動まで保持。"]),
    ("Проект сохранён", ["Project saved", "Projekt gespeichert", "프로젝트를 저장했습니다", "项目已保存", "プロジェクトを保存しました"]),
    ("Сохранён предыдущий снимок. Новые изменения ещё не сохранены.", ["An earlier snapshot was saved. New changes are not saved yet.", "Ein früherer Stand wurde gespeichert. Neue Änderungen sind noch ungespeichert.", "이전 상태를 저장했습니다. 새 변경 사항은 아직 저장되지 않았습니다.", "已保存先前的快照。新的更改尚未保存。", "以前の状態を保存しました。新しい変更は未保存です。"]),
    ("PNG сохранён. Проект со слоями сохраняется отдельно.", ["PNG saved. Save the layered project separately.", "PNG gespeichert. Das Ebenenprojekt wird separat gespeichert.", "PNG를 저장했습니다. 레이어가 있는 프로젝트는 별도로 저장하세요.", "PNG 已保存。含图层的项目需单独保存。", "PNGを保存しました。レイヤー付きプロジェクトは別途保存してください。"]),
    ("Некоторые изображения не удалось импортировать.", ["Some images could not be imported.", "Einige Bilder konnten nicht importiert werden.", "일부 이미지를 가져오지 못했습니다.", "部分图像无法导入。", "一部の画像を読み込めませんでした。"]),
    ("Можно перетащить до 32 изображений за один раз.", ["You can drop up to 32 images at once.", "Sie können bis zu 32 Bilder gleichzeitig ablegen.", "한 번에 최대 32개의 이미지를 끌어올 수 있습니다.", "一次最多可拖入 32 张图像。", "一度に最大32枚の画像をドロップできます。"]),
    ("Фиолетовый", ["Violet", "Violett", "보라색", "紫色", "バイオレット"]),
    ("Синий", ["Blue", "Blau", "파란색", "蓝色", "ブルー"]),
    ("Коралловый", ["Coral", "Koralle", "코랄", "珊瑚色", "コーラル"]),
    ("Янтарный", ["Amber", "Bernstein", "호박색", "琥珀色", "アンバー"]),
    ("Красный", ["Red", "Rot", "빨간색", "红色", "レッド"]),
    ("Размер изображения не совпадает с количеством пикселей.", ["Image dimensions do not match the pixel count.", "Bildgröße und Pixelanzahl stimmen nicht überein.", "이미지 크기와 픽셀 수가 일치하지 않습니다.", "图像尺寸与像素数量不一致。", "画像サイズとピクセル数が一致しません。"]),
    ("Выберите слой.", ["Select a layer.", "Wählen Sie eine Ebene.", "레이어를 선택하세요.", "请选择图层。", "レイヤーを選択してください。"]),
    ("Слой заблокирован. Снимите блокировку или выберите другой слой.", ["The layer is locked. Unlock it or choose another layer.", "Die Ebene ist gesperrt. Entsperren Sie sie oder wählen Sie eine andere.", "레이어가 잠겨 있습니다. 잠금을 해제하거나 다른 레이어를 선택하세요.", "图层已锁定。请解锁或选择其他图层。", "レイヤーはロック中です。解除するか別のレイヤーを選択してください。"]),
    ("Слой скрыт. Включите его видимость перед рисованием.", ["The layer is hidden. Show it before painting.", "Die Ebene ist verborgen. Blenden Sie sie vor dem Zeichnen ein.", "레이어가 숨겨져 있습니다. 그리기 전에 표시하세요.", "图层已隐藏。请先显示图层再绘画。", "レイヤーは非表示です。表示してから描画してください。"]),
    ("Достигнут лимит слоёв для этого размера холста (до 32 слоёв, до 256 МиБ пикселей).", ["Layer limit reached for this canvas size (up to 32 layers and 256 MiB of pixels).", "Ebenenlimit für diese Leinwand erreicht (bis 32 Ebenen und 256 MiB Pixeldaten).", "이 캔버스의 레이어 한도에 도달했습니다(최대 32개, 픽셀 데이터 256 MiB).", "已达到此画布的图层上限（最多 32 层、256 MiB 像素数据）。", "このキャンバスの上限に達しました（最大32レイヤー、256 MiBのピクセルデータ）。"]),
    ("В документе должен остаться хотя бы один слой.", ["The document must contain at least one layer.", "Das Dokument muss mindestens eine Ebene behalten.", "문서에는 최소 한 개의 레이어가 있어야 합니다.", "文档必须至少保留一个图层。", "ドキュメントには少なくとも1つのレイヤーが必要です。"]),
    ("Сначала снимите блокировку слоя.", ["Unlock the layer first.", "Entsperren Sie zuerst die Ebene.", "먼저 레이어 잠금을 해제하세요.", "请先解锁图层。", "先にレイヤーのロックを解除してください。"]),
    ("Слой не найден.", ["Layer not found.", "Ebene nicht gefunden.", "레이어를 찾을 수 없습니다.", "找不到图层。", "レイヤーが見つかりません。"]),
    ("Повреждён буфер изображения.", ["The image buffer is invalid.", "Der Bildpuffer ist beschädigt.", "이미지 버퍼가 손상되었습니다.", "图像缓冲区已损坏。", "画像バッファが破損しています。"]),
    ("Некорректные координаты штриха.", ["Invalid stroke coordinates.", "Ungültige Strichkoordinaten.", "획 좌표가 올바르지 않습니다.", "笔画坐标无效。", "ストロークの座標が無効です。"]),
    ("Некорректные координаты фигуры.", ["Invalid shape coordinates.", "Ungültige Formkoordinaten.", "도형 좌표가 올바르지 않습니다.", "形状坐标无效。", "図形の座標が無効です。"]),
    ("Файл Portfolio имеет недопустимый размер.", ["The Portfolio file has an invalid size.", "Die Portfolio-Datei hat eine ungültige Größe.", "Portfolio 파일 크기가 올바르지 않습니다.", "Portfolio 文件大小无效。", "Portfolioファイルのサイズが無効です。"]),
    ("Повреждён файл.", ["The file is damaged.", "Die Datei ist beschädigt.", "파일이 손상되었습니다.", "文件已损坏。", "ファイルが破損しています。"]),
    ("Файл повреждён: контрольная сумма не совпадает.", ["File damaged: checksum mismatch.", "Datei beschädigt: Prüfsumme stimmt nicht überein.", "파일이 손상되었습니다. 체크섬이 일치하지 않습니다.", "文件已损坏：校验和不匹配。", "ファイル破損：チェックサムが一致しません。"]),
    ("Это не поддерживаемый файл Portfolio версии 1.", ["This is not a supported Portfolio version 1 file.", "Dies ist keine unterstützte Portfolio-Datei der Version 1.", "지원되는 Portfolio 버전 1 파일이 아닙니다.", "这不是受支持的 Portfolio 版本 1 文件。", "対応するPortfolioバージョン1のファイルではありません。"]),
    ("В файле недопустимое количество слоёв.", ["The file contains an invalid number of layers.", "Die Datei enthält eine ungültige Ebenenanzahl.", "파일의 레이어 수가 올바르지 않습니다.", "文件中的图层数量无效。", "ファイルのレイヤー数が無効です。"]),
    ("Повреждены идентификаторы слоёв.", ["Layer identifiers are damaged.", "Ebenenkennungen sind beschädigt.", "레이어 식별자가 손상되었습니다.", "图层标识符已损坏。", "レイヤーIDが破損しています。"]),
    ("Повреждены параметры слоя.", ["Layer properties are damaged.", "Ebeneneigenschaften sind beschädigt.", "레이어 속성이 손상되었습니다.", "图层参数已损坏。", "レイヤーの設定が破損しています。"]),
    ("Недостаточно памяти для изображения.", ["Not enough memory for the image.", "Nicht genügend Speicher für das Bild.", "이미지를 위한 메모리가 부족합니다.", "没有足够的内存处理图像。", "画像用のメモリが不足しています。"]),
    ("В конце файла обнаружены неподдерживаемые данные.", ["Unsupported data found at the end of the file.", "Nicht unterstützte Daten am Dateiende gefunden.", "파일 끝에 지원되지 않는 데이터가 있습니다.", "文件末尾存在不支持的数据。", "ファイル末尾に未対応のデータがあります。"]),
    ("Недопустимая структура документа.", ["Invalid document structure.", "Ungültige Dokumentstruktur.", "문서 구조가 올바르지 않습니다.", "文档结构无效。", "ドキュメント構造が無効です。"]),
    ("Слишком длинное название документа.", ["The document title is too long.", "Der Dokumenttitel ist zu lang.", "문서 제목이 너무 깁니다.", "文档标题过长。", "ドキュメント名が長すぎます。"]),
    ("Недопустимая структура слоя.", ["Invalid layer structure.", "Ungültige Ebenenstruktur.", "레이어 구조가 올바르지 않습니다.", "图层结构无效。", "レイヤー構造が無効です。"]),
    ("Недостаточно памяти для нового слоя.", ["Not enough memory for a new layer.", "Nicht genügend Speicher für eine neue Ebene.", "새 레이어를 위한 메모리가 부족합니다.", "没有足够的内存创建新图层。", "新しいレイヤー用のメモリが不足しています。"]),
    ("Введите название слоя.", ["Enter a layer name.", "Geben Sie einen Ebenennamen ein.", "레이어 이름을 입력하세요.", "请输入图层名称。", "レイヤー名を入力してください。"]),
    ("Название слоя слишком длинное или содержит управляющие символы.", ["The layer name is too long or contains control characters.", "Der Ebenenname ist zu lang oder enthält Steuerzeichen.", "레이어 이름이 너무 길거나 제어 문자가 포함되어 있습니다.", "图层名称过长或包含控制字符。", "レイヤー名が長すぎるか、制御文字が含まれています。"]),
    ("Повреждён размер блока.", ["Invalid block size.", "Ungültige Blockgröße.", "블록 크기가 손상되었습니다.", "数据块大小已损坏。", "ブロックサイズが破損しています。"]),
    ("Файл обрывается раньше конца данных.", ["The file ends before all data is available.", "Die Datei endet vorzeitig.", "파일이 데이터 끝에 도달하기 전에 끝납니다.", "文件在数据结束前已中断。", "ファイルがデータの途中で終わっています。"]),
    ("Недопустимая длина текста в файле.", ["Invalid text length in the file.", "Ungültige Textlänge in der Datei.", "파일의 텍스트 길이가 올바르지 않습니다.", "文件中的文本长度无效。", "ファイル内のテキスト長が無効です。"]),
    ("В файле повреждён текст UTF-8.", ["The file contains damaged UTF-8 text.", "Die Datei enthält beschädigten UTF-8-Text.", "파일의 UTF-8 텍스트가 손상되었습니다.", "文件中的 UTF-8 文本已损坏。", "ファイル内のUTF-8テキストが破損しています。"]),
    ("В названии обнаружены управляющие символы.", ["The name contains control characters.", "Der Name enthält Steuerzeichen.", "이름에 제어 문자가 포함되어 있습니다.", "名称包含控制字符。", "名前に制御文字が含まれています。"]),
    ("Файл превышает допустимый размер.", ["The file exceeds the supported size limit.", "Die Datei überschreitet die zulässige Größe.", "파일이 허용된 크기를 초과합니다.", "文件超过允许的大小。", "ファイルが許容サイズを超えています。"]),
    ("Недостаточно памяти для файла.", ["Not enough memory for the file.", "Nicht genügend Speicher für die Datei.", "파일을 위한 메모리가 부족합니다.", "没有足够的内存读取文件。", "ファイル用のメモリが不足しています。"]),
    ("Некорректный размер буфера PNG.", ["Invalid PNG buffer size.", "Ungültige PNG-Puffergröße.", "PNG 버퍼 크기가 올바르지 않습니다.", "PNG 缓冲区大小无效。", "PNGバッファのサイズが無効です。"]),
    ("Ожидается несжатый BMP 24 или 32 бит.", ["An uncompressed 24-bit or 32-bit BMP is required.", "Eine unkomprimierte 24- oder 32-Bit-BMP-Datei wird benötigt.", "압축되지 않은 24비트 또는 32비트 BMP가 필요합니다.", "需要未压缩的 24 位或 32 位 BMP。", "非圧縮の24ビットまたは32ビットBMPが必要です。"]),
    ("Поддерживаются несжатые BMP 24/32 бит с обычной RGB-разметкой.", ["Only uncompressed 24/32-bit BMP with standard RGB layout is supported.", "Nur unkomprimierte 24/32-Bit-BMP mit Standard-RGB-Anordnung werden unterstützt.", "표준 RGB 배열의 비압축 24/32비트 BMP만 지원합니다.", "仅支持采用标准 RGB 排列的未压缩 24/32 位 BMP。", "標準RGB配列の非圧縮24/32ビットBMPに対応しています。"]),
    ("Повреждён BMP.", ["The BMP is damaged.", "Die BMP-Datei ist beschädigt.", "BMP가 손상되었습니다.", "BMP 已损坏。", "BMPが破損しています。"]),
    ("BMP обрывается раньше конца данных.", ["The BMP ends before all pixel data is available.", "Die BMP-Datei endet vorzeitig.", "BMP가 픽셀 데이터 끝에 도달하기 전에 끝납니다.", "BMP 在像素数据结束前已中断。", "BMPがピクセルデータの途中で終わっています。"]),
    ("Нативное окно Portfolio доступно в этой версии для Windows 10/11 (x64).", ["This version of Portfolio has a native window for Windows 10/11 (x64).", "Diese Version von Portfolio bietet ein natives Fenster für Windows 10/11 (x64).", "이 버전의 Portfolio 기본 창은 Windows 10/11(x64)용입니다.", "此版本的 Portfolio 原生窗口适用于 Windows 10/11 (x64)。", "このバージョンのPortfolioのネイティブウィンドウはWindows 10/11（x64）用です。"]),
    ("Импорт изображений через Windows GDI+ доступен только в Windows.", ["Image import through Windows GDI+ is available only on Windows.", "Bildimport über Windows GDI+ ist nur unter Windows verfügbar.", "Windows GDI+를 통한 이미지 가져오기는 Windows에서만 사용할 수 있습니다.", "通过 Windows GDI+ 导入图像仅适用于 Windows。", "Windows GDI+による画像の読み込みはWindowsでのみ利用できます。"]),
    ("Не удалось создать буфер окна.", ["Could not create the window buffer.", "Fensterpuffer konnte nicht erstellt werden.", "창 버퍼를 만들 수 없습니다.", "无法创建窗口缓冲区。", "ウィンドウバッファを作成できませんでした。"]),
    ("Недостаточно памяти для буфера окна.", ["Not enough memory for the window buffer.", "Nicht genügend Speicher für den Fensterpuffer.", "창 버퍼를 위한 메모리가 부족합니다.", "没有足够的内存创建窗口缓冲区。", "ウィンドウバッファ用のメモリが不足しています。"]),
    ("Portfolio — студия рисования", ["Portfolio — painting studio", "Portfolio — Zeichenstudio", "Portfolio — 드로잉 스튜디오", "Portfolio — 绘画工作室", "Portfolio — ペイントスタジオ"]),
    ("Не удалось открыть диалог", ["Could not open the dialog", "Dialog konnte nicht geöffnet werden", "대화상자를 열 수 없습니다", "无法打开对话框", "ダイアログを開けませんでした"]),
    ("Не удалось открыть поле ввода", ["Could not open the input dialog", "Eingabedialog konnte nicht geöffnet werden", "입력 대화상자를 열 수 없습니다", "无法打开输入对话框", "入力ダイアログを開けませんでした"]),
    ("Буфер обмена", ["Clipboard", "Zwischenablage", "클립보드", "剪贴板", "クリップボード"]),
    ("Внутренняя ошибка интерфейса. Перезапустите Portfolio и откройте последний сохранённый проект.", ["Internal interface error. Restart Portfolio and open the last saved project.", "Interner Oberflächenfehler. Starten Sie Portfolio neu und öffnen Sie das zuletzt gespeicherte Projekt.", "인터페이스 내부 오류입니다. Portfolio를 다시 시작하고 마지막으로 저장한 프로젝트를 여세요.", "界面内部错误。请重启 Portfolio 并打开最后保存的项目。", "画面内部エラーです。Portfolioを再起動し、最後に保存したプロジェクトを開いてください。"]),
    ("Проекты Portfolio (*.portfolio)\0*.portfolio\0Все файлы\0*.*\0\0", ["Portfolio projects (*.portfolio)\0*.portfolio\0All files\0*.*\0\0", "Portfolio-Projekte (*.portfolio)\0*.portfolio\0Alle Dateien\0*.*\0\0", "Portfolio 프로젝트 (*.portfolio)\0*.portfolio\0모든 파일\0*.*\0\0", "Portfolio 项目 (*.portfolio)\0*.portfolio\0所有文件\0*.*\0\0", "Portfolioプロジェクト (*.portfolio)\0*.portfolio\0すべてのファイル\0*.*\0\0"]),
    ("Открыть проект Portfolio", ["Open Portfolio project", "Portfolio-Projekt öffnen", "Portfolio 프로젝트 열기", "打开 Portfolio 项目", "Portfolioプロジェクトを開く"]),
    ("Сохранить проект Portfolio", ["Save Portfolio project", "Portfolio-Projekt speichern", "Portfolio 프로젝트 저장", "保存 Portfolio 项目", "Portfolioプロジェクトを保存"]),
    ("Изображения (*.png;*.jpg;*.jpeg;*.bmp)\0*.png;*.jpg;*.jpeg;*.bmp\0Все файлы\0*.*\0\0", ["Images (*.png;*.jpg;*.jpeg;*.bmp)\0*.png;*.jpg;*.jpeg;*.bmp\0All files\0*.*\0\0", "Bilder (*.png;*.jpg;*.jpeg;*.bmp)\0*.png;*.jpg;*.jpeg;*.bmp\0Alle Dateien\0*.*\0\0", "이미지 (*.png;*.jpg;*.jpeg;*.bmp)\0*.png;*.jpg;*.jpeg;*.bmp\0모든 파일\0*.*\0\0", "图像 (*.png;*.jpg;*.jpeg;*.bmp)\0*.png;*.jpg;*.jpeg;*.bmp\0所有文件\0*.*\0\0", "画像 (*.png;*.jpg;*.jpeg;*.bmp)\0*.png;*.jpg;*.jpeg;*.bmp\0すべてのファイル\0*.*\0\0"]),
    ("Импортировать изображение", ["Import image", "Bild importieren", "이미지 가져오기", "导入图像", "画像を読み込む"]),
    ("Сохранить изображение", ["Save image", "Bild speichern", "이미지 저장", "保存图像", "画像を保存"]),
    ("Изображение PNG (*.png)\0*.png\0\0", ["PNG image (*.png)\0*.png\0\0", "PNG-Bild (*.png)\0*.png\0\0", "PNG 이미지 (*.png)\0*.png\0\0", "PNG 图像 (*.png)\0*.png\0\0", "PNG画像 (*.png)\0*.png\0\0"]),
    ("Открыть PNG", ["Open PNG", "PNG öffnen", "PNG 열기", "打开 PNG", "PNGを開く"]),
    ("Экспортировать PNG", ["Export PNG", "PNG exportieren", "PNG 내보내기", "导出 PNG", "PNGを書き出す"]),
    ("Применить", ["Apply", "Anwenden", "적용", "应用", "適用"]),
    ("Отмена", ["Cancel", "Abbrechen", "취소", "取消", "キャンセル"]),
    ("Буфер обмена занят другим приложением. Повторите копирование.", ["Another application is using the clipboard. Try copying again.", "Ein anderes Programm verwendet die Zwischenablage. Kopieren Sie erneut.", "다른 앱이 클립보드를 사용 중입니다. 다시 복사하세요.", "其他应用正在使用剪贴板。请重试复制。", "別のアプリがクリップボードを使用中です。もう一度コピーしてください。"]),
    ("Недостаточно памяти для копирования.", ["Not enough memory to copy.", "Nicht genügend Speicher zum Kopieren.", "복사를 위한 메모리가 부족합니다.", "没有足够的内存进行复制。", "コピー用のメモリが不足しています。"]),
    ("Не удалось подготовить текст для копирования.", ["Could not prepare the text for copying.", "Text konnte nicht zum Kopieren vorbereitet werden.", "복사할 텍스트를 준비할 수 없습니다.", "无法准备要复制的文本。", "コピーするテキストを準備できませんでした。"]),
    ("Не удалось передать текст в буфер обмена.", ["Could not place text on the clipboard.", "Text konnte nicht in die Zwischenablage übertragen werden.", "텍스트를 클립보드에 넣을 수 없습니다.", "无法将文本放入剪贴板。", "テキストをクリップボードに入れられませんでした。"]),
    ("Не удалось запустить системный декодер изображений.", ["Could not start the system image decoder.", "System-Bilddecoder konnte nicht gestartet werden.", "시스템 이미지 디코더를 시작할 수 없습니다.", "无法启动系统图像解码器。", "システムの画像デコーダーを起動できませんでした。"]),
    ("Не удалось прочитать изображение. Используйте корректный PNG, JPEG или BMP.", ["Could not read the image. Use a valid PNG, JPEG or BMP.", "Bild konnte nicht gelesen werden. Verwenden Sie eine gültige PNG-, JPEG- oder BMP-Datei.", "이미지를 읽을 수 없습니다. 올바른 PNG, JPEG 또는 BMP를 사용하세요.", "无法读取图像。请使用有效的 PNG、JPEG 或 BMP。", "画像を読み込めません。正しいPNG、JPEG、BMPを使用してください。"]),
    ("Не удалось определить размер изображения.", ["Could not determine image dimensions.", "Bildgröße konnte nicht bestimmt werden.", "이미지 크기를 확인할 수 없습니다.", "无法确定图像尺寸。", "画像サイズを取得できませんでした。"]),
    ("Изображение должно быть от 1×1 до 4096×4096 пикселей. Уменьшите его перед импортом.", ["The image must be between 1×1 and 4096×4096 pixels. Resize it before importing.", "Das Bild muss 1×1 bis 4096×4096 Pixel groß sein. Verkleinern Sie es vor dem Import.", "이미지 크기는 1×1~4096×4096픽셀이어야 합니다. 가져오기 전에 크기를 줄이세요.", "图像必须介于 1×1 至 4096×4096 像素之间。请先缩小再导入。", "画像は1×1～4096×4096ピクセルにしてください。読み込み前に縮小してください。"]),
    ("Системный декодер не смог получить пиксели изображения.", ["The system decoder could not read image pixels.", "Der Systemdecoder konnte die Bildpixel nicht lesen.", "시스템 디코더가 이미지 픽셀을 읽을 수 없습니다.", "系统解码器无法读取图像像素。", "システムデコーダーが画像のピクセルを取得できませんでした。"]),
    ("Декодер вернул некорректные данные изображения.", ["The decoder returned invalid image data.", "Der Decoder lieferte ungültige Bilddaten.", "디코더가 잘못된 이미지 데이터를 반환했습니다.", "解码器返回了无效的图像数据。", "デコーダーが無効な画像データを返しました。"]),
    ("Фоновая операция прервана. Повторите действие.", ["The background operation was interrupted. Try again.", "Der Hintergrundvorgang wurde unterbrochen. Versuchen Sie es erneut.", "백그라운드 작업이 중단되었습니다. 다시 시도하세요.", "后台操作已中断。请重试。", "バックグラウンド処理が中断されました。再試行してください。"]),
    ("Не удалось запустить фоновую операцию.", ["Could not start the background operation.", "Hintergrundvorgang konnte nicht gestartet werden.", "백그라운드 작업을 시작할 수 없습니다.", "无法启动后台操作。", "バックグラウンド処理を開始できませんでした。"]),
    ("За один раз можно добавить до 32 изображений.", ["You can add up to 32 images at once.", "Sie können bis zu 32 Bilder gleichzeitig hinzufügen.", "한 번에 최대 32개의 이미지를 추가할 수 있습니다.", "一次最多可添加 32 张图像。", "一度に最大32枚の画像を追加できます。"]),
    ("Импорт изображений", ["Image import", "Bilder importieren", "이미지 가져오기", "导入图像", "画像の読み込み"]),
    ("Не удалось обновить окно", ["Could not refresh the window", "Fenster konnte nicht aktualisiert werden", "창을 갱신할 수 없습니다", "无法刷新窗口", "ウィンドウを更新できませんでした"]),
    ("Не удалось подключить буфер окна.", ["Could not attach the window buffer.", "Fensterpuffer konnte nicht eingebunden werden.", "창 버퍼를 연결할 수 없습니다.", "无法连接窗口缓冲区。", "ウィンドウバッファを接続できませんでした。"]),
    ("Не удалось прочитать путь перетаскиваемого файла.", ["Could not read the dropped file path.", "Der Pfad der abgelegten Datei konnte nicht gelesen werden.", "끌어온 파일 경로를 읽을 수 없습니다.", "无法读取拖入文件的路径。", "ドロップしたファイルのパスを読み取れませんでした。"]),
    ("Недостаточно памяти для импорта изображения.", ["Not enough memory to import the image.", "Nicht genügend Speicher für den Bildimport.", "이미지를 가져올 메모리가 부족합니다.", "没有足够的内存导入图像。", "画像を読み込むためのメモリが不足しています。"]),
    ("Portfolio · меню проекта", ["Portfolio · project menu", "Portfolio · Projektmenü", "Portfolio · 프로젝트 메뉴", "Portfolio · 项目菜单", "Portfolio · プロジェクトメニュー"]),
    ("Акцент", ["Accent", "Akzentfarbe", "강조색", "强调色", "アクセント"]),
    ("Тема", ["Theme", "Design", "테마", "主题", "テーマ"]),
    ("Добавляем изображения…", ["Adding images…", "Bilder werden hinzugefügt…", "이미지 추가 중…", "正在添加图像…", "画像を追加中…"]),
    ("Достигнут предел числа слоёв.", ["The layer limit has been reached.", "Die maximale Ebenenanzahl wurde erreicht.", "레이어 수 한도에 도달했습니다.", "已达到图层数量上限。", "レイヤー数の上限に達しました。"]),
    ("Можно добавить не более 32 изображений за одну операцию.", ["You can add up to 32 images in one operation.", "Sie können höchstens 32 Bilder pro Vorgang hinzufügen.", "한 번에 최대 32개의 이미지를 추가할 수 있습니다.", "一次操作最多可添加 32 张图像。", "1回の操作で追加できる画像は最大32枚です。"]),
    ("Резервная копия не записана: фоновая операция прервана.", ["Recovery copy not saved: the background operation was interrupted.", "Sicherung nicht gespeichert: Hintergrundvorgang wurde unterbrochen.", "복구 사본을 저장하지 못했습니다. 백그라운드 작업이 중단되었습니다.", "未保存备份：后台操作已中断。", "バックアップ未保存：バックグラウンド処理が中断されました。"]),
    ("Фоновая операция прервана. Попробуйте ещё раз.", ["The background operation was interrupted. Please try again.", "Der Hintergrundvorgang wurde unterbrochen. Bitte versuchen Sie es erneut.", "백그라운드 작업이 중단되었습니다. 다시 시도해 주세요.", "后台操作已中断。请重试。", "バックグラウンド処理が中断されました。再試行してください。"]),
    ("Недостаточно памяти для копии документа.", ["Not enough memory for a document snapshot.", "Nicht genügend Speicher für eine Dokumentkopie.", "문서 사본을 위한 메모리가 부족합니다.", "没有足够的内存创建文档快照。", "ドキュメントのコピー用メモリが不足しています。"]),
    ("Повреждён буфер слоя.", ["The layer buffer is damaged.", "Der Ebenenpuffer ist beschädigt.", "레이어 버퍼가 손상되었습니다.", "图层缓冲区已损坏。", "レイヤーバッファが破損しています。"]),
];
const TEMPLATES: &[Template] = &[
    Template {
        source: "{0} × {1} px  ·  {2} сл.  ·  {3}",
        translated: [
            "{0} × {1} px  ·  {2} layers  ·  {3}",
            "{0} × {1} px  ·  {2} Ebenen  ·  {3}",
            "{0} × {1} px  ·  레이어 {2}개  ·  {3}",
            "{0} × {1} px  ·  {2} 层  ·  {3}",
            "{0} × {1} px  ·  {2}レイヤー  ·  {3}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Добавлено изображений: {0}. Ошибок: {1}.",
        translated: [
            "Images added: {0}. Errors: {1}.",
            "Bilder hinzugefügt: {0}. Fehler: {1}.",
            "추가한 이미지: {0}. 오류: {1}.",
            "已添加图像：{0}。错误：{1}。",
            "追加した画像：{0}。エラー：{1}。",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Введите число от {0} до {1}.",
        translated: [
            "Enter a number from {0} to {1}.",
            "Geben Sie eine Zahl von {0} bis {1} ein.",
            "{0}~{1} 사이의 숫자를 입력하세요.",
            "请输入 {0} 至 {1} 之间的数字。",
            "{0}～{1}の数値を入力してください。",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Страница {0} из {1}",
        translated: [
            "Page {0} of {1}",
            "Seite {0} von {1}",
            "{1}페이지 중 {0}페이지",
            "第 {0} 页，共 {1} 页",
            "{1}ページ中{0}ページ",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Страница {0} / {1}",
        translated: [
            "Page {0} / {1}",
            "Seite {0} / {1}",
            "페이지 {0} / {1}",
            "第 {0} / {1} 页",
            "ページ{0} / {1}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Размер холста должен быть от 1 до {0} пикселей по каждой стороне.",
        translated: [
            "Canvas sides must be between 1 and {0} pixels.",
            "Leinwandseiten müssen zwischen 1 und {0} Pixel lang sein.",
            "캔버스의 각 변은 1~{0}픽셀이어야 합니다.",
            "画布每边必须为 1 至 {0} 像素。",
            "キャンバスの各辺は1～{0}ピクセルにしてください。",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Диалог Windows вернул ошибку {0}. Повторите действие.",
        translated: [
            "Windows dialog returned error {0}. Try again.",
            "Windows-Dialog meldet Fehler {0}. Versuchen Sie es erneut.",
            "Windows 대화상자 오류 {0}. 다시 시도하세요.",
            "Windows 对话框返回错误 {0}。请重试。",
            "Windowsダイアログでエラー{0}が発生しました。再試行してください。",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Не удалось открыть Portfolio (ошибка Windows {0}).",
        translated: [
            "Could not open Portfolio (Windows error {0}).",
            "Portfolio konnte nicht geöffnet werden (Windows-Fehler {0}).",
            "Portfolio를 열 수 없습니다(Windows 오류 {0}).",
            "无法打开 Portfolio（Windows 错误 {0}）。",
            "Portfolioを開けませんでした（Windowsエラー{0}）。",
        ],
        ui_slots: 0,
    },
    Template {
        source: "{0}: стрелки — изменить, Enter — точное значение",
        translated: [
            "{0}: arrows to adjust, Enter for exact value",
            "{0}: Pfeile zum Ändern, Enter für genaue Eingabe",
            "{0}: 방향키로 조절, Enter로 정확한 값 입력",
            "{0}：方向键调整，Enter 精确输入",
            "{0}：矢印で調整、Enterで正確な値",
        ],
        ui_slots: 1,
    },
    Template {
        source: "Не удалось зарегистрировать окно Windows ({0}).",
        translated: [
            "Could not register the Windows window ({0}).",
            "Windows-Fenster konnte nicht registriert werden ({0}).",
            "Windows 창을 등록할 수 없습니다({0}).",
            "无法注册 Windows 窗口（{0}）。",
            "Windowsウィンドウを登録できませんでした（{0}）。",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Не удалось запустить фоновую операцию: {0}",
        translated: [
            "Could not start background operation: {0}",
            "Hintergrundvorgang konnte nicht gestartet werden: {0}",
            "백그라운드 작업을 시작할 수 없습니다: {0}",
            "无法启动后台操作：{0}",
            "バックグラウンド処理を開始できませんでした：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Не удалось прочитать параметры файла: {0}",
        translated: [
            "Could not read file metadata: {0}",
            "Dateieigenschaften konnten nicht gelesen werden: {0}",
            "파일 정보를 읽을 수 없습니다: {0}",
            "无法读取文件属性：{0}",
            "ファイル情報を読み取れませんでした：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Windows не смог открыть поле ввода ({0}).",
        translated: [
            "Windows could not open the input dialog ({0}).",
            "Windows konnte den Eingabedialog nicht öffnen ({0}).",
            "Windows가 입력 대화상자를 열지 못했습니다({0}).",
            "Windows 无法打开输入对话框（{0}）。",
            "Windowsが入力ダイアログを開けませんでした（{0}）。",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Не удалось импортировать изображение: {0}",
        translated: [
            "Could not import image: {0}",
            "Bild konnte nicht importiert werden: {0}",
            "이미지를 가져올 수 없습니다: {0}",
            "无法导入图像：{0}",
            "画像を読み込めませんでした：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Ошибка очереди сообщений Windows ({0}).",
        translated: [
            "Windows message queue error ({0}).",
            "Fehler in der Windows-Nachrichtenwarteschlange ({0}).",
            "Windows 메시지 큐 오류({0}).",
            "Windows 消息队列错误（{0}）。",
            "Windowsメッセージキューのエラー（{0}）。",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Не удалось сохранить настройки: {0}",
        translated: [
            "Could not save settings: {0}",
            "Einstellungen konnten nicht gespeichert werden: {0}",
            "설정을 저장할 수 없습니다: {0}",
            "无法保存设置：{0}",
            "設定を保存できませんでした：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Не удалось экспортировать PNG: {0}",
        translated: [
            "Could not export PNG: {0}",
            "PNG konnte nicht exportiert werden: {0}",
            "PNG를 내보낼 수 없습니다: {0}",
            "无法导出 PNG：{0}",
            "PNGを書き出せませんでした：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Не удалось подготовить снимок: {0}",
        translated: [
            "Could not prepare snapshot: {0}",
            "Momentaufnahme konnte nicht vorbereitet werden: {0}",
            "스냅샷을 준비할 수 없습니다: {0}",
            "无法准备快照：{0}",
            "スナップショットを準備できませんでした：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Не удалось сохранить проект: {0}",
        translated: [
            "Could not save project: {0}",
            "Projekt konnte nicht gespeichert werden: {0}",
            "프로젝트를 저장할 수 없습니다: {0}",
            "无法保存项目：{0}",
            "プロジェクトを保存できませんでした：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Резервная копия не записана: {0}",
        translated: [
            "Recovery copy was not saved: {0}",
            "Sicherung wurde nicht gespeichert: {0}",
            "복구 사본을 저장하지 못했습니다: {0}",
            "未能保存备份：{0}",
            "バックアップを保存できませんでした：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Не удалось прочитать файл: {0}",
        translated: [
            "Could not read file: {0}",
            "Datei konnte nicht gelesen werden: {0}",
            "파일을 읽을 수 없습니다: {0}",
            "无法读取文件：{0}",
            "ファイルを読み取れませんでした：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Не удалось открыть файл: {0}",
        translated: [
            "Could not open file: {0}",
            "Datei konnte nicht geöffnet werden: {0}",
            "파일을 열 수 없습니다: {0}",
            "无法打开文件：{0}",
            "ファイルを開けませんでした：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Добавлено изображений: {0}",
        translated: [
            "Images added: {0}",
            "Bilder hinzugefügt: {0}",
            "추가한 이미지: {0}",
            "已添加图像：{0}",
            "追加した画像：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Цвет взят с холста: {0}",
        translated: [
            "Color sampled from canvas: {0}",
            "Farbe von der Leinwand aufgenommen: {0}",
            "캔버스에서 선택한 색상: {0}",
            "已从画布取色：{0}",
            "キャンバスから取得した色：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Размер текста: {0} px",
        translated: [
            "Text size: {0} px",
            "Textgröße: {0} px",
            "텍스트 크기: {0} px",
            "文字大小：{0} px",
            "文字サイズ：{0} px",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Слой: {0} · F2 — имя",
        translated: [
            "Layer: {0} · F2: rename",
            "Ebene: {0} · F2: umbenennen",
            "레이어: {0} · F2: 이름 변경",
            "图层：{0} · F2：重命名",
            "レイヤー：{0} · F2：名前変更",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Сбросить значение: {0}",
        translated: [
            "Reset {0}",
            "{0} zurücksetzen",
            "{0} 초기화",
            "重置{0}",
            "{0}をリセット",
        ],
        ui_slots: 1,
    },
    Template {
        source: "Точное значение: {0}",
        translated: [
            "Exact value: {0}",
            "Genauer Wert: {0}",
            "정확한 값: {0}",
            "精确数值：{0}",
            "正確な値：{0}",
        ],
        ui_slots: 1,
    },
    Template {
        source: "Размер текста: {0}%",
        translated: [
            "Text size: {0}%",
            "Textgröße: {0}%",
            "텍스트 크기: {0}%",
            "文字大小：{0}%",
            "文字サイズ：{0}%",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Импортируем: {0}",
        translated: [
            "Importing: {0}",
            "Importieren: {0}",
            "가져오는 중: {0}",
            "正在导入：{0}",
            "読み込み中：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Недавний {0}",
        translated: [
            "Recent {0}",
            "Zuletzt {0}",
            "최근 색상 {0}",
            "最近使用 {0}",
            "最近の色 {0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "СЛОИ  /  {0}",
        translated: [
            "LAYERS  /  {0}",
            "EBENEN  /  {0}",
            "레이어  /  {0}",
            "图层  /  {0}",
            "レイヤー  /  {0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Акцент: {0}",
        translated: [
            "Accent: {0}",
            "Akzent: {0}",
            "강조색: {0}",
            "强调色：{0}",
            "アクセント：{0}",
        ],
        ui_slots: 1,
    },
    Template {
        source: "{0} — копия",
        translated: [
            "{0} — copy",
            "{0} — Kopie",
            "{0} — 사본",
            "{0} — 副本",
            "{0} — コピー",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Шрифт: {0}",
        translated: [
            "Font: {0}",
            "Schrift: {0}",
            "글꼴: {0}",
            "字体：{0}",
            "フォント：{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Цвет #{0}",
        translated: [
            "Color #{0}",
            "Farbe #{0}",
            "색상 #{0}",
            "颜色 #{0}",
            "カラー #{0}",
        ],
        ui_slots: 0,
    },
    Template {
        source: "Слой {0}",
        translated: [
            "Layer {0}",
            "Ebene {0}",
            "레이어 {0}",
            "图层 {0}",
            "レイヤー{0}",
        ],
        ui_slots: 0,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn language_keys_and_native_names_roundtrip() {
        for language in Language::ALL {
            assert_eq!(Language::from_key(language.key()), Some(language));
            assert!(!language.label().is_empty());
        }
        assert_eq!(Language::from_key("ZH_HANS"), Some(Language::Chinese));
        assert_eq!(Language::from_key("unknown"), None);
    }

    #[test]
    fn catalogue_entries_are_complete_and_unique() {
        let mut keys = HashSet::new();
        for (source, translations) in CATALOG {
            assert!(keys.insert(*source), "duplicate: {source}");
            assert!(!source.is_empty());
            for target in translations {
                assert!(!target.is_empty(), "empty translation: {source}");
                assert_ne!(source, target, "untranslated: {source}");
            }
        }
    }

    #[test]
    fn unknown_text_paths_and_inserted_braces_are_preserved() {
        let path = "C:\\Рисунки\\Кисть {1} — копия.png";
        for language in Language::ALL {
            assert_eq!(language.translate(path), path);
            let text = format!("Не удалось открыть файл: {path}");
            let localized = language.translate(&text);
            assert!(localized.ends_with(path));
            assert!(localized.contains("{1}"));
        }
    }

    #[test]
    fn templates_preserve_every_interpolated_value_once() {
        for template in TEMPLATES {
            let count = (0..8)
                .take_while(|n| template.source.contains(&format!("{{{n}}}")))
                .count();
            let values: Vec<String> = (0..count).map(|n| format!("SLOT{n}_{{1}}_雨")).collect();
            let source = substitute(template.source, &values);
            assert!(
                capture(template.source, &source).is_some(),
                "{}",
                template.source
            );
            for language in Language::ALL.into_iter().skip(1) {
                let translated = language.translate(&source);
                for value in &values {
                    assert_eq!(
                        translated.matches(value).count(),
                        1,
                        "{} => {translated}",
                        template.source
                    );
                }
            }
        }
    }

    #[test]
    fn file_filters_preserve_nul_delimiters_and_extensions() {
        let source = "Проекты Portfolio (*.portfolio)\0*.portfolio\0Все файлы\0*.*\0\0";
        for language in Language::ALL {
            let target = language.translate(source);
            assert_eq!(source.matches('\0').count(), target.matches('\0').count());
            assert!(target.contains("*.portfolio\0"));
            assert!(target.ends_with("*.*\0\0"));
        }
    }
}
