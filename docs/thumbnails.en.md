# Content thumbnails

[日本語](thumbnails.ja.md) | English

Choose `.ai`, `.eps`, `.svg`, `.psd`, `.psb` and `.pdf` independently in **Content thumbnails**. Select **Review changes** to inspect the extensions being enabled or restored and the affected registration locations, then choose **Apply selection**. When an association is shared, the review includes the resulting display for unselected extensions. If the selection or registration changes after review, review it again. Returning to the previous screen keeps the draft; **Discard changes** restores the current registration as the selection. Each row's applied state reflects the checked Windows registration. Icon registration, default applications and file contents are not changed.

| Extension | Preview source |
| --- | --- |
| `.ai` | An embedded TIFF or XMP image. Rendering PDF-compatible artwork is not currently supported. |
| `.eps` | An embedded TIFF, EPSI or XMP image. PostScript is not rendered. |
| `.svg` | The SVG is rendered without fetching external resources. |
| `.psd`, `.psb` | A supported embedded thumbnail. |
| `.pdf` | The first page, rendered by Windows. |

No content image is returned when the required data is absent or the file exceeds format or size limits. To disable registration, deselect an extension and apply the selection, or choose **Restore all**. Thumbnail restoration is independent of icon restoration.

For shared associations, the app records the observed members and previous thumbnail registrations so unselected extensions can retain their previous providers. If a default application, shared association or registration changes externally, the app preserves the recovery record instead of overwriting the change. The app stops when a machine-level shared registration cannot be safely suppressed for the user. If an operation fails, inspect the details and retry **Restore all**. Conflicting groups are retained while other groups that can be restored are restored first.

An extension without a default application is recorded in that state before only its thumbnail is registered. If a default application is subsequently set, the change is detected and the old state is not restored automatically.

When the last group is disabled, the original registration is restored, the empty selection is saved, and only then is the recovery record removed. A save or removal failure keeps the record for retry. If a saved selection exists but its recovery record is missing, the previous registration cannot be inferred, so new application and clearing the settings are stopped.

Explorer display, compatibility with existing handlers, cloud file retrieval and speed vary by environment and file. The app does not guarantee that every file contains a preview or that a particular display speed will be achieved.
