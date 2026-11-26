# Иконка Mcrate

Чёрно-белая версия: волнистая «печенька» (M3 Expressive cookie), кнопка Play и нота.

- `icon.svg` — основная: чёрная печенька (#111111), белый знак. Исходник.
- `icon_light.svg` / `icon_light_1024.png` — инверсия: белая печенька, чёрный знак (для тёмных панелей).
- `icon_256.rgba` — то, что вшивается в окно (`include_bytes!` в `main.rs`). Сейчас собрано из основной.
- `icon.ico` — для .exe и ярлыков Windows (16–256 px).
- `icon_*.png` — для README, сайта, упаковщиков.

Чтобы окно использовало светлую версию, пересоберите RGBA из `icon_light.svg`:
    python -c "import cairosvg;from PIL import Image;cairosvg.svg2png(url='icon_light.svg',write_to='t.png',output_width=256,output_height=256);open('icon_256.rgba','wb').write(Image.open('t.png').convert('RGBA').tobytes())"
- `tray_black_32.rgba` / `tray_white_32.rgba` — иконка в трее (32x32, сырой RGBA, `include_bytes!` в `tray.rs`). Чёрная для светлой панели задач, белая для тёмной. Пересборка:
    python -c "from PIL import Image;[open(d,'wb').write(Image.open(s).convert('RGBA').resize((32,32),Image.LANCZOS).tobytes()) for s,d in (('icon_1024.png','tray_black_32.rgba'),('icon_light_1024.png','tray_white_32.rgba'))]"
