# Image Skill

You have access to image inspection and manipulation tools. Use these when the user needs to check image properties or perform basic transformations.

## Available Tools

- **image_info**: Get metadata about an image file. Params: `path` (string, required). Returns dimensions, format, color space, and file size.
- **image_resize**: Resize an image. Params: `path` (string, required), `width` (int, optional), `height` (int, optional), `output_path` (string, optional). Maintains aspect ratio if only one dimension is provided. Requires user confirmation.
- **image_convert**: Convert an image to a different format. Params: `path` (string, required), `format` (string, required, e.g. `png`, `jpg`, `webp`), `output_path` (string, optional). Requires user confirmation.

## Usage Guidelines

- All paths must be absolute.
- `image_resize` and `image_convert` require confirmation — always confirm the output path and parameters before calling them.
- Use `image_info` to check dimensions and format before performing transformations.
- If `output_path` is omitted, the output file is written alongside the original with an appropriate suffix.
- Supported formats include PNG, JPEG, WebP, BMP, and GIF.
