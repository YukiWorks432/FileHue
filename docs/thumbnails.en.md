# Content thumbnails

[日本語](thumbnails.ja.md) | English

Choose `.ai`, `.eps`, `.svg`, `.psd`, `.psb` and `.pdf` independently in **File previews**. The examples compare the file icon with a sample content thumbnail. Each card shows the checked registration separately from any unapplied selection. Choose **Apply changes** to apply the selection. The app checks registrations during application and stops rather than overwriting conflicts. Leaving with unapplied changes asks for confirmation; **Discard changes** restores the current registration as the selection. Icon registration, default applications and file contents are not changed.

| Extension | Preview source |
| --- | --- |
| `.ai` | An embedded TIFF or XMP image. Rendering PDF-compatible artwork is not currently supported. |
| `.eps` | An embedded TIFF, EPSI or XMP image. PostScript is not rendered. |
| `.svg` | The SVG is rendered without fetching external resources. |
| `.psd`, `.psb` | A supported embedded thumbnail. |
| `.pdf` | The first page, rendered by Windows. |

No content image is returned when the required data is absent or the file exceeds format or size limits. To disable registration, deselect an extension and apply the selection, or choose **Restore previous settings** and confirm. Thumbnail restoration is independent of icon restoration.

For shared associations, the app records the observed members and previous thumbnail registrations so unselected extensions can retain their previous providers. If a default application, shared association or registration changes externally, the app preserves the recovery record instead of overwriting the change. The app stops when a machine-level shared registration cannot be safely suppressed for the user. If an operation fails, inspect the details and retry **Restore previous settings**. Conflicting groups are retained while other groups that can be restored are restored first.

An extension without a default application is recorded in that state before only its thumbnail is registered. If a default application is subsequently set, the change is detected and the old state is not restored automatically.

When restoring an extension-only registration, an added user-level extension value that explicitly names the same previously effective ProgID is preserved while only the thumbnail is restored. This requires the effective association and all other references to remain unchanged, and every thumbnail registration to match either the applied or original value. This exception does not cover a newly assigned default application, a different application or reference, or a group that changes a shared class. New application and applied-state checks still require an exact match with the recorded references.

When the last group is disabled, the original registration is restored, the empty selection is saved, and only then is the recovery record removed. A save or removal failure keeps the record for retry. If a saved selection exists but its recovery record is missing, the previous registration cannot be inferred, so new application and clearing the settings are stopped.

Explorer display, compatibility with existing handlers, cloud file retrieval and speed vary by environment and file. The app does not guarantee that every file contains a preview or that a particular display speed will be achieved.

If an interrupted shared-setting change conflicts, its group and pending transition are preserved while independent groups are restored first. Restoration can be retried from the same record after resolving the conflict.
