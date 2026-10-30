//! Japanese labels for the GPUI components used by Editor. GPUI 0.8 has no Japanese catalog.
use std::sync::{LazyLock, Once};

pub(super) fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| gpui_kit::component::_rust_i18n_extend(&*JAPANESE, "component"));
}

static JAPANESE: LazyLock<rust_i18n_support::SimpleBackend> = LazyLock::new(|| {
    let mut backend = rust_i18n_support::SimpleBackend::new();
    backend.add_translations(
        "ja".into(),
        LABELS
            .iter()
            .map(|(key, value)| (format!("component.{key}").into(), (*value).into()))
            .collect(),
    );
    backend
});

const LABELS: &[(&str, &str)] = &[
    ("Select.placeholder", "選択してください"),
    ("ComboBox.placeholder", "選択してください"),
    ("ComboBox.search_placeholder", "検索…"),
    ("ComboBox.empty", "結果はありません"),
    ("DropdownButton.more_options", "その他の操作"),
    ("Dock.Unnamed", "名称未設定"),
    ("Dock.Close", "閉じる"),
    ("Dock.Zoom In", "拡大"),
    ("Dock.Zoom Out", "縮小"),
    ("Dock.Collapse", "折りたたむ"),
    ("Dock.Expand", "展開"),
    ("ColorPicker.Palette", "パレット"),
    ("ColorPicker.HSLA", "HSLA"),
    ("ColorPicker.Hue", "色相"),
    ("ColorPicker.Saturation", "彩度"),
    ("ColorPicker.Lightness", "明度"),
    ("ColorPicker.Alpha", "不透明度"),
    ("Dialog.ok", "OK"),
    ("Dialog.cancel", "キャンセル"),
    ("Command.placeholder", "コマンドを入力または検索…"),
    ("Command.empty", "結果はありません"),
    ("List.search_placeholder", "検索…"),
    ("Input.Decrement", "減らす"),
    ("Input.Increment", "増やす"),
    ("Input.Replace", "置換"),
    ("Input.Replace All", "すべて置換"),
    ("Input.Clear", "クリア"),
    ("Input.Cut", "切り取り"),
    ("Input.Copy", "コピー"),
    ("Input.Paste", "貼り付け"),
    ("Input.Select All", "すべて選択"),
    ("Input.Go to Definition", "定義に移動"),
    ("Input.Show Code Actions", "コードアクションを表示"),
    ("Input.Show Password", "パスワードを表示"),
    ("Input.Hide Password", "パスワードを非表示"),
    ("Settings.search_placeholder", "検索…"),
    ("Settings.Reset All", "すべてリセット"),
    ("Pagination.previous", "前へ"),
    ("Pagination.next", "次へ"),
    ("Table.SortBy", "%{column}で並べ替え"),
];
