# Portfolio

Студия 2D-рисования для Windows: кисти, слои и панели, которые можно расположить под себя.

**Windows 10 (1703+) / 11 · x64 · Версия 0.1.0**

[**Скачать Portfolio.exe**](https://github.com/GBB-C2/Portfolio/raw/refs/heads/main/Portfolio.exe) · [Начало работы](START_HERE_RU.md)

![Рабочее пространство Portfolio с холстом, панелью инструментов и выбором цвета](previews/Portfolio_Quick_Controls.png)

| Настройки кисти | Работа со слоями |
| :---: | :---: |
| [![Каталог кистей и параметры рисования](previews/Portfolio_Brush_Properties_Basic.png)](previews/Portfolio_Brush_Properties_Basic.png) | [![Панель слоёв: видимость, блокировка и непрозрачность](previews/Portfolio_Layers.png)](previews/Portfolio_Layers.png) |

*Программные превью интерфейса. Отрисовка шрифтов в Windows может отличаться.*

## Возможности

- 10 кистей, 10 ластиков, заливка, пипетка и геометрические фигуры.
- Слои, отмена и повтор, зеркальное рисование, масштабирование холста.
- Импорт PNG/JPEG/BMP, экспорт PNG, сохранение проектов и резервное восстановление.
- Перемещаемые панели, две темы, шесть языков и настройка шрифтов.

## Быстрый запуск

Скачайте **Portfolio.exe** по ссылке выше и запустите. Rust и установка шрифтов не нужны.

При запуске открыт этюд FORM 01 — можно сразу рисовать. **Ctrl+N** создаёт новый холст, **Ctrl+S** сохраняет проект, **Ctrl+Shift+E** экспортирует PNG.

## Сборка из исходников

На Windows нужны Rust (`x86_64-pc-windows-msvc`), MSVC Build Tools и Windows SDK. Выполните из корня проекта:

```powershell
cargo run --release
```

[Полное руководство](docs/USER_GUIDE_RU.md) · [Проверки и ограничения](docs/VALIDATION.md) · [Лицензия MIT](LICENSE)
