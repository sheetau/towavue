# Drag badge artwork

The 23-DIP badge decoration retains Monapad `src/cursor.html` styling (MIT; see [license](MONAPAD-LICENSE.txt)): 75% white background, gray border and 5-DIP corners. Its 17-DIP symbols now use the pinned [Fluent artwork](../fluent/README.md): Arrow Right, Window New and Prohibited. Forbidden uses red; the other symbols use black. The native decoration follows the pointer at a 12-DIP offset and does not intercept input. Rasterization is cached by state and pixel size, using the already locked tiny-skia 0.11.4. It is not a browser or an OLE drag image.
