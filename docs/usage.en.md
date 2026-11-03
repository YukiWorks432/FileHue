# Usage

The GUI and auxiliary CLI target Windows 11 x64. See the [0.1.0 overview](releases/0.1.0.en.md) for supported scope and limitations. Downloading and previewing do not change Shell registration. Applying affects matching files for the current user, so review restoration before applying.

Select “About” to see the application version and open the application and third-party licenses. Individual icon-pack terms are available in each pack's details.

## Before launching

This build targets Windows 11 x64 and requires the Microsoft Visual C++ v14 Redistributable (x64), at least as recent as the MSVC 14.44 build tools used for this build. If the runtime is missing or Windows reports that VCRUNTIME140.dll cannot be found, use [Microsoft's official download instructions](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170). The application does not install the runtime automatically. Extract the ZIP and keep the executable and Shell DLL in the same folder.

The initial distribution is unsigned. Check the official distribution source and compare the ZIP's SHA-256 with `SHA256SUMS.txt` obtained from the same source. In PowerShell, calculate it with `Get-FileHash -Algorithm SHA256 <path-to-ZIP>`, replacing `<path-to-ZIP>` with the actual path. If the values differ, do not extract or run it; check the source again. A hash checks contents and does not authenticate the publisher as a code signature does. SmartScreen may display a warning; Smart App Control or organizational execution controls may prevent the application from starting. Trusting a self-signed certificate or broadly disabling protection is not required.

## Download and preview

1. Open **Browse packs** to see authors, reviewed versions, sources, and license summaries. The external-link icon, **Open source and license information**, opens the author's repository in your browser.
2. Choose **Download**. Progress shows downloading, verification and extraction, then icon conversion. Choose **Cancel** to request cancellation and wait for it to finish. Read error details and retry if needed.
3. Switch to **Downloaded packs** and open **Preview and licenses** on a downloaded pack. The preview opens directly below that pack. Use the same button or Esc to close it. Examples use the actual filename and extension rules. Type a filename to try it. Unsupported filenames indicate the Windows default icon fallback.
4. Expand **Source and licenses (original text)** to read the notices. The view displays up to 32,000 characters and lists the full provenance file's location.

Catppuccin Icons adds four color variants. Dedicated icons depend on the selected theme; a special filename is not guaranteed to have a distinct icon.

Samples reflect saved target selections, without icon protection. **Check planned icon** opens the comparison for that filename, including protection. **Saved** uses saved settings; **Draft** also reflects unsaved target and protection changes. Navigation does not save or apply settings.

Rules referencing undefined icons are skipped with a warning. Check the warning count in the downloaded library and open **Preview and licenses** for details. The provenance file keeps the complete record. Symbols is pinned to reviewed version 0.0.7; version 0.0.26 contains rules outside the application's supported scope.

To import a local theme, choose **Import pack…** and select an unpacked VS Code extension folder containing `package.json`. Choose a theme if several are available. **Cancel import** dismisses the pending choices. The source folder is preserved.

During import, the footer shows **Preparing and converting pack** and **Cancel**. Once conversion starts, it shows the current theme number, completed and total image definitions, and elapsed time for the whole operation. Rules sharing an image definition count once; image counts restart for each theme. No download bytes, estimated percentage or remaining time are shown for local imports. Choose **Cancel** to request cancellation and wait for it to finish; a single image conversion may not stop immediately. Cancellation before saving preserves the pending choices, existing packs, target selections and applied icons, so you can retry the same choice. Closing the window also requests cancellation. If saving has already completed, the successful result is preserved.

## Supported images and limits

PNG, JPEG, SVG, and images inside ICO files are limited to 4,096 pixels per side. SVG input is limited to 8 MiB of UTF-8 XML, 20,000 XML nodes, and 512 KiB of attribute values and text excluding embedded data URLs. Compressed SVG and DTDs are rejected. Embedded PNG, JPEG, and child SVG images share a 16,777,216-pixel budget, with at most four SVG levels including the root. ICO input is limited to 32 frames, 32 MiB of stored frame data, and 16,777,216 total pixels. Frame spans and dimensions are checked before decoding. PNG and JPEG decoders receive a 64 MiB allocation budget; this is not a process-wide memory limit. An import that exceeds these limits fails without changing the applied pack.

To rebuild a pack downloaded by an earlier development build, remove it and download again, or re-import the original extension folder. Restore Windows defaults first if the pack is active. Previously prepared files are not overwritten automatically.

## Apply, update, and restore

When another version of the same theme is installed, **Review settings from another version…** opens the existing copy workflow. Review the source and destination versions and matching, added and removed target counts. Expand **Review added and removed targets** to see their names. **Inherit matching settings** copies saved choices into the draft, keeps destination-only targets and supports Undo. Use **Save selection** or **Apply this selection** to commit. Older packs and their settings remain available; downloading and copying do not switch the active pack.

Choose **Apply** to change Explorer registration. Download completion never applies a pack automatically. The active pack is identified, and reapplying or removing it is disabled. Apply another pack to switch.

If icons do not update after applying or restoring, finish file operations such as copying or moving, restart **Windows Explorer** in Task Manager, and reopen your folders. A restart may be needed for the first registration. The application never closes Explorer automatically. Restoration puts the previous registration values back; it cannot repair original icon files that another application update has already removed.

**Download update** appears when the reviewed catalog contains a newer version. Download and preview it before applying. The application does not automatically discover or apply the upstream latest version.

**Restore Windows defaults** restores the registration that preceded installation. It does not remove downloaded packs or uninstall the application. If a failed switch successfully restores the previous pack, the status explains that outcome. If recovery also fails, its record is retained: read the details before retrying restoration.

## Updating FileHue

**Download update** updates an icon pack, not FileHue itself. Obtain a new application ZIP manually. Preserve the old application and back up settings, packs, and recovery records before updating. Check the new version's notes for saved-format compatibility. Unsupported formats stop normal operations; do not assume an older version can read settings written by a newer one. Before returning to an older version, check its compatibility and the preserved data.

To back up the data, close the GUI and CLI, then copy `%LOCALAPPDATA%\WindowsFileIconsDev` to a separate safe location. While recovery records remain, do not move or delete the folder or overwrite current settings with a backup.

1. In the old version, choose **Restore Windows defaults**, confirm that restoration completed, and close the application. If a conflict or failure is reported, retain the recovery records and resolve it before continuing; see [Recover from registration conflicts](#recover-from-registration-conflicts).
2. Verify the source and SHA-256 of the new ZIP and extract it to a different folder. Keep the GUI, CLI, and `file_icons_shell.dll` together. Do not overwrite files in use.
3. Start the new version and review downloaded packs, target selections, and protection settings. Choose **Apply** to apply the desired pack again. Launching the new version alone does not update the Shell DLL in use.

Reapplying after restoration checks existing icon registrations again. If another application's update or another change has altered an icon reference, a saved choice to use the pack is marked **review needed**, and the original icon is protected until you confirm it again. Open **Icon protection** to review the registration and preview, then select and save only the types you want to change.

If an operation fails, retain its details and recovery records. Keep the old version and backups until application or restoration is confirmed complete. If a DLL cannot be cleaned up because it is in use, close applications using it or sign in to Windows again, then retry. FileHue does not forcibly close Explorer.

## Removal and settings

Switch away from an active pack or restore Windows defaults before choosing **Remove…**. Check the pack name and removal scope, then choose **Remove** to proceed. **Cancel** or Esc keeps the pack and its files. Shared files and files needed for restoration are preserved. When files are in use or unknown files are present, the removal request is saved. Resolve the reported issue, such as closing an application holding a file or moving your own files elsewhere, then choose **Retry removing remaining pack files**. Original import folders and staged Shell DLLs are preserved.

After restoring Windows defaults, choose **Clean up unused app files** to remove staged Shell DLLs from the managed storage. Existing registration or unfinished recovery records prevent cleanup. Files in use, modified DLLs, unknown files, and links are preserved. Read the details, close applications holding the files or sign in to Windows again, and retry. The application never forcibly closes Explorer. This action preserves packs, settings, recovery backups, and files in the folder containing the running application.

To remove the application, first choose **Restore Windows defaults**, then review the result of **Clean up unused app files**. Close the application and delete its extracted folder. Settings, packs, and recovery backups in the user's managed storage remain after deleting that folder. If you choose to remove those separately, first confirm restoration and preserve any backups you need. The application cannot recreate deleted recovery records.

Choose **日本語** or **English** to save the display language for future launches. **Toggle appearance** changes the current window's appearance.

Damaged settings disable normal changes. If an independent recovery record is available, **Back up settings and restore** preserves the original settings bytes and restores registration. The pack list and language are retained when their separate management file is readable; a damaged pack list is not repaired.

Use the sun or moon icon in the header to switch appearance. Tooltips explain supplementary icon actions. Registration status and operation results remain visible while the pack list scrolls.

## Choose icon targets

**Include** selects the pack's scope; existing-icon protection can still take priority. Each row shows the planned result, and **Protected** filters protected targets. Expand **Protection** in a row to compare the original and pack images and choose **Original icon**, **Pack icon**, or **Use software setting**. These protection rules affect all packs and have separate save controls at the bottom. Closing the target editor discards unsaved protection changes while retaining the target-selection draft.

Search is limited to the selected category and state. TypeScript's `.mts` is under **Web**, so it will not appear when searching only **Programming languages**. If results are empty, **Search all targets** broadens the scope while keeping the query. If the entire pack has no matching definition, the editor explains that separately. Protection never hides targets from the list.

Use **Choose targets…** on an installed pack. Filter by category, search, enabled state, overrides or changes. Toggle individual targets, all matching results, a whole category, or selected rows. Filename overrides take priority over extension settings. Disabled targets use the original Windows icons rather than a less specific icon from the pack. **Inherit** removes an individual override.

Categories describe a target's purpose and are shared across packs. The app combines its own classification table with bundled VS Code language definitions, without network access. Specific names take priority: `package.json` is under **Build and dependencies**, while `tsconfig.json` is under **Data and configuration**. ZIP and tar.gz targets appear under **Archives and compressed files**. Unknown or conflicting types remain unclassified. Classification updates preserve saved target choices; category-wide edits use the current membership.

Edits remain drafts until **Save selection**, **Apply changes**, or **Apply this selection**. Compare a filename against saved and draft settings before applying. Undo keeps the last 32 edits; **Revert to saved** discards the draft. Drafts survive switching between packs in the current window. Copying from another saved theme includes only matching identifiers and shows the unmatched count. Disabling every target restores the previous Windows registration. Errors preserve the draft for retry.

For the active pack, **Apply changes** is the only commit action. It updates the selection without applying the same pack a second time. For an inactive pack, **Save selection** saves without changing Explorer, while **Apply this selection** explicitly applies the draft. Disabling all targets in the active pack restores the previous registration.

The CLI supports `targets list <pack-id>`, `targets edit <pack-id> <scope> <on|off|inherit>`, `targets set <pack-id> <changes.json>`, and `targets copy <source-id> <destination-id>`. Scopes include `all`, `category:data`, `query:json`, and an exact identifier such as `file:package.json`. Changes support `--dry-run`. The UTF-8 changes file (up to 1 MiB) maps identifiers to true, false, or null (inherit). Unknown targets reject the entire change. JSON output and error identifiers remain language independent.

## Recover from registration conflicts

Open **Review conflicts** in the header. Choose **Restore and preserve external changes** or **Reapply the icon pack**, review the preview, choose **Review changes…**, check the backup location, then execute. Selecting a recovery method alone makes no changes. Unavailable reapplication is disabled with a reason. Reapplication uses the saved selection and preserves unsaved drafts. External settings become the new restore baseline, so a later normal restore returns to them. A changed state invalidates the preview; refresh it before retrying. Incomplete operations keep their record and can be reviewed after restarting. See [CLI recovery commands](cli.en.md#confirmed-conflict-recovery).

## Protected file types

**Icon protection** opens the application-wide **Protect existing icons by default** setting, enabled by default. Choose a pack to see both protected file types and those allowed to use a pack. Use **Protected**, **Allowed**, **Review**, or **Excluded** to filter the list. Edited rows stay visible until you select a filter again. Open **Registration details** for classes and registry locations; **Copy details** includes complete values even when the display is truncated. **Save and reapply** restores the previous registration before applying the new policy; conflicts stop the operation. Returning without saving discards the draft. All-excluded packs write no registration and can still be restored. Pack-card previews describe theme rules; inspect protection here or in **Choose targets…** to understand exclusions. Settings use runtime and library format 4. Unsupported formats are not overwritten automatically; independent recovery records in formats 1 and 2 remain readable.

`.lnk` and `.exe` always retain their Windows-managed icons. Their target checkboxes are disabled; bulk edits and copies cannot enable them. Explicit CLI enabling fails without saving any changes. `targets list --json` exposes `protected`; `resolve --json` returns `reason: protected_file_type`. To remove registrations left by an older version, explicitly apply the theme again. Existing external changes are reported as conflicts. Restart Explorer if its old DLL or icon cache remains in use. Launching the app alone does not change registration.

## Protection by provider and file type

Open **Existing icon protection** and select a pack. Under **Software defaults**, select **Original icon**, **Pack icon**, or **Use global setting**. File type overrides take priority and remain intact when the software default changes. The software-level selection remains visible alongside the override count. Choose **Use software setting** for a file type to remove its override. If no software rule exists, the global setting applies. File types sharing an association can still use independent settings. Changes remain drafts until **Save** or **Save and reapply**. Returning discards unsaved changes.

Click anywhere in a software card heading, including its icon, name, count, and padding, or focus it with Tab and press Enter, to collapse or expand its file types. Bulk actions are inside the expanded content. Use **Expand all** and **Collapse all** at the right end of the toolbar immediately above the list to open or close all cards. Settings and unsaved changes are retained; reopen any card from its heading. A software icon appears beside the provider name when available.

Select **Original icon**, **Pack icon**, or **Use software setting** using equally sized tiles. Only the selected tile has a check, an accent border, and a full-brightness image. Images in unselected tiles are dimmed. The inheritance tile shows the icon that would be used with the override removed and updates with draft changes. Clicking the selected tile keeps it selected. Each file type also shows its saved choice and representative current and after-saving previews. A result that cannot be determined is marked **Unavailable**. Previews use a sample filename; the actual display may vary with the filename or Explorer's refresh state. Rules saved for another pack remain visible and are marked when the selected pack does not cover them. You can inspect and change saved rules even after removing all packs. Unavailable previews use a warning icon. Choose **Refresh** to inspect external changes. Changed registrations require review. Shortcuts and executables remain protected. Use **Reset software settings…** to clear both the software rule and its inspected file type overrides. The warning names the software and shows the override count. Cancel or press Escape to keep the draft. Confirming selects **Use global setting** for the software and **Use software setting** for its file types. Other software settings are kept, and the reset takes effect only after saving.

Packaged Windows apps use their resolved display names and application icons. Resources belonging to the same verified application ID share one card; each file type retains its own original preview. Earlier resource-level permissions do not automatically become permissions for the whole app. Review the three choices when a registration change is reported.
