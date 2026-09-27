# scene-navigation 0.1.0

This workspace crate is owned and maintained by this repository. Native and WASM
hosts share it locally; building, testing, and changing it requires no other checkout
or cross-repository synchronization.

Camera coordinates are right-handed, Y-up. Speeds are world units/second. The
platform adapters produce the same input frame; camera math never accesses a GPU.
Physical keyboard bindings are mutable through `CameraController.input.bindings`.

See [controls and verification](CONTROLS.md) for usage and acceptance checks.

Copyright (c) 2022-2026 Hugo Woodiwiss

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in the
Software without restriction, including without limitation the rights to use,
copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the
Software, and to permit persons to whom the Software is furnished to do so,
subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
