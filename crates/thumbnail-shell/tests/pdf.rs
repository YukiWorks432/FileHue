#![cfg(windows)]

#[path = "../../../test-support/process.rs"]
#[allow(dead_code)] // この試験では子プロセスへ個別の待機期限を指定する。
mod test_process;

use std::{ffi::c_void, os::windows::ffi::OsStrExt, process::Command, time::Duration};
use test_process::CommandOutput;
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE, S_FALSE, S_OK},
        Graphics::Gdi::{DIBSECTION, DeleteObject, GetObjectW, HBITMAP, HGDIOBJ},
        System::{
            Com::{
                COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
                IClassFactory, IStream,
            },
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            WinRT::{
                CreateStreamOverRandomAccessStream, RO_INIT_MULTITHREADED, RO_INIT_SINGLETHREADED,
                RoInitialize, RoUninitialize,
            },
        },
        UI::Shell::{
            IThumbnailProvider, PropertiesSystem::IInitializeWithStream, SHCreateMemStream,
            WTS_ALPHATYPE, WTSAT_ARGB,
        },
    },
    core::{GUID, HRESULT, Interface, PCSTR, PCWSTR, Result},
};

#[test]
fn first_page_is_rendered_in_sta_and_mta() {
    for apartment in ["sta", "mta"] {
        for child in ["pdf_child", "provider_com_child"] {
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", child, "--nocapture"])
                .env("WFI_THUMBNAIL_APARTMENT", apartment)
                .test_output_with_timeout(Duration::from_secs(30))
                .unwrap();
            assert!(
                output.status.success(),
                "{apartment} {child}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
#[ignore = "親試験がCOMアパートメントと期限を指定して起動する"]
fn provider_com_child() -> Result<()> {
    let mode = match std::env::var("WFI_THUMBNAIL_APARTMENT").as_deref() {
        Ok("sta") => COINIT_APARTMENTTHREADED,
        Ok("mta") => COINIT_MULTITHREADED,
        _ => panic!("parent must specify the apartment"),
    };
    unsafe { CoInitializeEx(None, mode).ok()? };
    let result = (|| {
        let bitmap = provider_bitmap(file_icons_thumbnail_core::format::Format::Pdf, &red_pdf())?;
        verify_bitmap(bitmap, 256, 128, &[0, 0, 255, 255])?;
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><rect width="1" height="1" fill="#ff0000" fill-opacity="0.5"/></svg>"##;
        let bitmap = provider_bitmap(file_icons_thumbnail_core::format::Format::Svg, svg)?;
        verify_bitmap(bitmap, 1, 1, &[0, 0, 128, 128])?;
        for (format, version) in [
            (file_icons_thumbnail_core::format::Format::Psd, 1u16),
            (file_icons_thumbnail_core::format::Format::Psb, 2u16),
        ] {
            let bitmap = provider_bitmap(format, &photoshop_red_preview(version))?;
            verify_bitmap(bitmap, 1, 1, &[0, 0, 255, 255])?;
        }
        let epsi = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BeginPreview: 1 1 1 1\n%80\n%%EndPreview\n";
        let bitmap = provider_bitmap(file_icons_thumbnail_core::format::Format::Eps, epsi)?;
        verify_bitmap(bitmap, 1, 1, &[0, 0, 0, 255])?;
        for format in [
            file_icons_thumbnail_core::format::Format::Ai,
            file_icons_thumbnail_core::format::Format::Eps,
        ] {
            let bitmap = provider_bitmap(format, &white_xmp_preview())?;
            verify_bitmap(bitmap, 1, 1, &[255, 255, 255, 255])?;
        }
        for (format, data) in [
            (
                file_icons_thumbnail_core::format::Format::Psd,
                photoshop_without_preview(1),
            ),
            (
                file_icons_thumbnail_core::format::Format::Psb,
                photoshop_without_preview(2),
            ),
            (
                file_icons_thumbnail_core::format::Format::Eps,
                b"%!PS-Adobe-3.0 EPSF-3.0\n%%EndComments\n".to_vec(),
            ),
            (
                file_icons_thumbnail_core::format::Format::Ai,
                b"%PDF-1.7\nno embedded thumbnail".to_vec(),
            ),
            // 有効なPDFでも、AIの作品内容と確認できないページは表示しない。
            (file_icons_thumbnail_core::format::Format::Ai, red_pdf()),
        ] {
            assert!(
                provider_bitmap(format, &data).is_err(),
                "{format:?} must not return a fabricated thumbnail"
            );
        }
        Ok(())
    })();
    unsafe { CoUninitialize() };
    result
}

fn photoshop_red_preview(version: u16) -> Vec<u8> {
    let mut payload = Vec::new();
    for value in [0u32, 1, 1, 4, 4, 4] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload.extend_from_slice(&24u16.to_be_bytes());
    payload.extend_from_slice(&1u16.to_be_bytes());
    payload.extend_from_slice(&[255, 0, 0, 0]);
    let mut resource = b"8BIM".to_vec();
    resource.extend_from_slice(&1036u16.to_be_bytes());
    resource.extend_from_slice(&[0, 0]);
    resource.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    resource.extend_from_slice(&payload);
    let mut document = b"8BPS".to_vec();
    document.extend_from_slice(&version.to_be_bytes());
    document.extend_from_slice(&[0; 20]);
    document.extend_from_slice(&0u32.to_be_bytes());
    document.extend_from_slice(&(resource.len() as u32).to_be_bytes());
    document.extend_from_slice(&resource);
    document
}

fn photoshop_without_preview(version: u16) -> Vec<u8> {
    let mut document = b"8BPS".to_vec();
    document.extend_from_slice(&version.to_be_bytes());
    document.extend_from_slice(&[0; 20]);
    document.extend_from_slice(&0u32.to_be_bytes());
    document.extend_from_slice(&0u32.to_be_bytes());
    document
}

fn white_xmp_preview() -> Vec<u8> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode(&[255, 255, 255], 1, 1, image::ExtendedColorType::Rgb8)
        .unwrap();
    let image = STANDARD.encode(jpeg);
    format!(r#"%!PS-Adobe-3.0 EPSF-3.0
<m:xmpmeta xmlns:m="adobe:ns:meta/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:x="http://ns.adobe.com/xap/1.0/" xmlns:i="http://ns.adobe.com/xap/1.0/g/img/"><rdf:RDF><rdf:Description><x:Thumbnails><rdf:Alt><rdf:li i:format="JPEG" i:image="{image}"/></rdf:Alt></x:Thumbnails></rdf:Description></rdf:RDF></m:xmpmeta>"#).into_bytes()
}

#[test]
#[ignore = "親試験がアパートメントと期限を指定して起動する"]
fn pdf_child() -> Result<()> {
    let mode = match std::env::var("WFI_THUMBNAIL_APARTMENT").as_deref() {
        Ok("sta") => RO_INIT_SINGLETHREADED,
        Ok("mta") => RO_INIT_MULTITHREADED,
        _ => panic!("parent must specify the apartment"),
    };
    unsafe { RoInitialize(mode)? };
    let result = render_and_compare();
    unsafe { RoUninitialize() };
    result
}

fn render_and_compare() -> Result<()> {
    let data = red_pdf();
    let input = unsafe { SHCreateMemStream(Some(&data)).unwrap() };
    let output = file_icons_thumbnail_shell::pdf::render_first_page(&input, 256)?;
    let length = output.Size()? as usize;
    assert!(length > 0 && length < 1024 * 1024);
    let stream: IStream = unsafe { CreateStreamOverRandomAccessStream(&output)? };
    let mut png = vec![0; length];
    let mut read = 0;
    unsafe {
        stream
            .Read(png.as_mut_ptr().cast(), length as u32, Some(&mut read))
            .ok()?
    };
    assert_eq!(read as usize, length);
    let image = image::load_from_memory(&png).unwrap().to_rgba8();
    let (width, height) = image.dimensions();
    assert!(width > 0 && height > 0);
    assert!((i64::from(width) - 2 * i64::from(height)).abs() <= 1);
    assert_eq!(image.get_pixel(width / 2, height / 2).0, [255, 0, 0, 255]);
    output.Seek(0)?;
    let stream: IStream = unsafe { CreateStreamOverRandomAccessStream(&output)? };
    let bitmap = file_icons_thumbnail_shell::raster::decode(
        &stream,
        file_icons_thumbnail_shell::raster::Format::Png,
        256,
    )?;
    assert_eq!((bitmap.width, bitmap.height), (256, 128));
    let index = ((64 * bitmap.width + 128) * 4) as usize;
    assert_eq!(&bitmap.bytes[index..index + 4], &[0, 0, 255, 255]);
    output.Close()?;
    verify_transparency()?;
    verify_jpeg_and_tiff()?;
    let handle = provider_bitmap(file_icons_thumbnail_core::format::Format::Pdf, &data)?;
    verify_bitmap(handle, 256, 128, &[0, 0, 255, 255])?;
    Ok(())
}

fn provider_bitmap(
    format: file_icons_thumbnail_core::format::Format,
    data: &[u8],
) -> Result<HBITMAP> {
    struct Library(HMODULE);
    impl Drop for Library {
        fn drop(&mut self) {
            let _ = unsafe { FreeLibrary(self.0) };
        }
    }
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("file_icons_thumbnail_shell.dll");
    let name: Vec<u16> = dll.as_os_str().encode_wide().chain(Some(0)).collect();
    let library = Library(unsafe { LoadLibraryW(PCWSTR(name.as_ptr()))? });
    let get =
        unsafe { GetProcAddress(library.0, PCSTR(c"DllGetClassObject".as_ptr().cast())) }.unwrap();
    let unload =
        unsafe { GetProcAddress(library.0, PCSTR(c"DllCanUnloadNow".as_ptr().cast())) }.unwrap();
    let get: unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT =
        unsafe { std::mem::transmute(get) };
    let unload: unsafe extern "system" fn() -> HRESULT = unsafe { std::mem::transmute(unload) };
    let clsid = GUID::from_u128(format.clsid());
    let mut raw = std::ptr::null_mut();
    unsafe { get(&clsid, &IClassFactory::IID, &mut raw).ok()? };
    let factory = unsafe { IClassFactory::from_raw(raw) };
    assert_eq!(unsafe { unload() }, S_FALSE);
    let provider: IInitializeWithStream = unsafe { factory.CreateInstance(None)? };
    let input = unsafe { SHCreateMemStream(Some(data)).unwrap() };
    unsafe { provider.Initialize(&input, 0)? };
    assert!(unsafe { provider.Initialize(&input, 0) }.is_err());
    drop(input);
    let thumbnail: IThumbnailProvider = provider.cast()?;
    let mut handle = HBITMAP::default();
    let mut alpha = WTS_ALPHATYPE::default();
    unsafe { thumbnail.GetThumbnail(256, &mut handle, &mut alpha)? };
    assert_eq!(alpha, WTSAT_ARGB);
    drop(thumbnail);
    drop(provider);
    drop(factory);
    assert_eq!(unsafe { unload() }, S_OK);
    Ok(handle)
}

fn verify_bitmap(handle: HBITMAP, width: i32, height: i32, first_pixel: &[u8]) -> Result<()> {
    let mut section = DIBSECTION::default();
    let size = std::mem::size_of::<DIBSECTION>();
    let count = unsafe {
        GetObjectW(
            HGDIOBJ(handle.0),
            size as i32,
            Some((&mut section as *mut DIBSECTION).cast()),
        )
    };
    assert_eq!(count as usize, size);
    assert_eq!(section.dsBm.bmWidth, width);
    assert_eq!(section.dsBm.bmHeight, height);
    assert_eq!(section.dsBm.bmBitsPixel, 32);
    let data = unsafe { std::slice::from_raw_parts(section.dsBm.bmBits.cast::<u8>(), 4) };
    assert_eq!(data, first_pixel);
    assert!(unsafe { DeleteObject(HGDIOBJ(handle.0)) }.as_bool());
    Ok(())
}

fn verify_transparency() -> Result<()> {
    use image::ImageEncoder;

    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &[255, 0, 0, 128, 0, 0, 0, 0],
            2,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    let stream = unsafe { SHCreateMemStream(Some(&png)).unwrap() };
    let bitmap = file_icons_thumbnail_shell::raster::decode(
        &stream,
        file_icons_thumbnail_shell::raster::Format::Png,
        256,
    )?;
    assert_eq!((bitmap.width, bitmap.height), (2, 1));
    assert_eq!(bitmap.bytes, [0, 0, 128, 128, 0, 0, 0, 0]);
    let handle = bitmap.to_bitmap()?;
    let mut section = DIBSECTION::default();
    let size = std::mem::size_of::<DIBSECTION>();
    let count = unsafe {
        GetObjectW(
            HGDIOBJ(handle.0),
            size as i32,
            Some((&mut section as *mut DIBSECTION).cast()),
        )
    };
    assert_eq!(count as usize, size);
    assert_eq!(section.dsBm.bmWidth, 2);
    assert_eq!(section.dsBm.bmHeight, 1);
    assert_eq!(section.dsBm.bmBitsPixel, 32);
    let data = unsafe { std::slice::from_raw_parts(section.dsBm.bmBits.cast::<u8>(), 8) };
    assert_eq!(data, bitmap.bytes);
    assert!(unsafe { DeleteObject(HGDIOBJ(handle.0)) }.as_bool());
    Ok(())
}

fn verify_jpeg_and_tiff() -> Result<()> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use file_icons_thumbnail_shell::raster::{Format, decode};

    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
        .encode(&[255, 0, 0], 1, 1, image::ExtendedColorType::Rgb8)
        .unwrap();
    let stream = unsafe { SHCreateMemStream(Some(&jpeg)).unwrap() };
    let bitmap = decode(&stream, Format::Jpeg, 128)?;
    assert_eq!((bitmap.width, bitmap.height), (1, 1));
    assert!(bitmap.bytes[0] < 10 && bitmap.bytes[1] < 10 && bitmap.bytes[2] > 245);
    assert_eq!(bitmap.bytes[3], 255);
    let expected_jpeg_pixel = bitmap.bytes[..4].to_vec();
    let encoded = STANDARD.encode(&jpeg);
    let xml = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:base="http://ns.adobe.com/xap/1.0/" xmlns:img="http://ns.adobe.com/xap/1.0/g/img/"><rdf:RDF><rdf:Description><base:Thumbnails><rdf:Alt><rdf:li><img:format>JPEG</img:format><img:image>{encoded}</img:image></rdf:li></rdf:Alt></base:Thumbnails></rdf:Description></rdf:RDF></x:xmpmeta>"#
    );
    for (format, header) in [
        (
            file_icons_thumbnail_core::format::Format::Eps,
            "%!PS-Adobe-3.0 EPSF-3.0\n",
        ),
        (file_icons_thumbnail_core::format::Format::Ai, "%PDF-1.7\n"),
    ] {
        let document = format!("{header}{xml}");
        let handle = provider_bitmap(format, document.as_bytes())?;
        verify_bitmap(handle, 1, 1, &expected_jpeg_pixel)?;
    }

    let document = photoshop_document(1036, 1, &jpeg);
    let stream = unsafe { SHCreateMemStream(Some(&document)).unwrap() };
    let mut reader = file_icons_thumbnail_shell::stream::StreamReader::new(stream);
    let thumbnail = file_icons_thumbnail_core::photoshop::extract(&mut reader)
        .unwrap()
        .unwrap();
    let bitmap = file_icons_thumbnail_shell::raster::photoshop(&thumbnail, 128)?;
    assert_eq!((bitmap.width, bitmap.height), (1, 1));
    assert!(bitmap.bytes[0] < 10 && bitmap.bytes[1] < 10 && bitmap.bytes[2] > 245);

    let document = photoshop_document(1033, 0, &[0, 0, 255, 0]);
    let stream = unsafe { SHCreateMemStream(Some(&document)).unwrap() };
    let mut reader = file_icons_thumbnail_shell::stream::StreamReader::new(stream);
    let thumbnail = file_icons_thumbnail_core::photoshop::extract(&mut reader)
        .unwrap()
        .unwrap();
    let bitmap = file_icons_thumbnail_shell::raster::photoshop(&thumbnail, 128)?;
    assert_eq!(bitmap.bytes, [0, 0, 255, 255]);
    let document = photoshop_document(1033, 0, &[0, 0, 255, 0]);
    let handle = provider_bitmap(file_icons_thumbnail_core::format::Format::Psd, &document)?;
    verify_bitmap(handle, 1, 1, &[0, 0, 255, 255])?;
    let mut large_document = document;
    large_document[4..6].copy_from_slice(&2u16.to_be_bytes());
    let handle = provider_bitmap(
        file_icons_thumbnail_core::format::Format::Psb,
        &large_document,
    )?;
    verify_bitmap(handle, 1, 1, &[0, 0, 255, 255])?;

    let tiff = red_tiff();
    let stream = unsafe { SHCreateMemStream(Some(&tiff)).unwrap() };
    let bitmap = decode(&stream, Format::Tiff, 128)?;
    assert_eq!((bitmap.width, bitmap.height), (1, 1));
    assert_eq!(bitmap.bytes, [0, 0, 255, 255]);
    let mut eps = vec![0; 30];
    eps[..4].copy_from_slice(&[0xc5, 0xd0, 0xd3, 0xc6]);
    eps[20..24].copy_from_slice(&30u32.to_le_bytes());
    eps[24..28].copy_from_slice(&(tiff.len() as u32).to_le_bytes());
    eps.extend_from_slice(&tiff);
    let stream = unsafe { SHCreateMemStream(Some(&eps)).unwrap() };
    let mut reader = file_icons_thumbnail_shell::stream::StreamReader::new(stream);
    let extracted = file_icons_thumbnail_core::eps::extract_tiff(&mut reader)
        .unwrap()
        .unwrap();
    let stream = unsafe { SHCreateMemStream(Some(&extracted)).unwrap() };
    let bitmap = decode(&stream, Format::Tiff, 128)?;
    assert_eq!(bitmap.bytes, [0, 0, 255, 255]);
    let mut eps = vec![0; 30];
    eps[..4].copy_from_slice(&[0xc5, 0xd0, 0xd3, 0xc6]);
    eps[20..24].copy_from_slice(&30u32.to_le_bytes());
    eps[24..28].copy_from_slice(&(tiff.len() as u32).to_le_bytes());
    eps.extend_from_slice(&tiff);
    let handle = provider_bitmap(file_icons_thumbnail_core::format::Format::Eps, &eps)?;
    verify_bitmap(handle, 1, 1, &[0, 0, 255, 255])?;
    let epsi = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BeginPreview: 2 2 1 2\n%80\n%40\n%%EndPreview\n";
    let handle = provider_bitmap(file_icons_thumbnail_core::format::Format::Eps, epsi)?;
    verify_bitmap(handle, 2, 2, &[255, 255, 255, 255])?;
    let mut dos_epsi = vec![0; 30];
    dos_epsi[..4].copy_from_slice(&[0xc5, 0xd0, 0xd3, 0xc6]);
    dos_epsi[4..8].copy_from_slice(&30u32.to_le_bytes());
    dos_epsi[8..12].copy_from_slice(&(epsi.len() as u32).to_le_bytes());
    dos_epsi.extend_from_slice(epsi);
    let handle = provider_bitmap(file_icons_thumbnail_core::format::Format::Eps, &dos_epsi)?;
    verify_bitmap(handle, 2, 2, &[255, 255, 255, 255])?;
    Ok(())
}

fn photoshop_document(id: u16, format: u32, image: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    for value in [format, 1, 1, 4, 4, image.len() as u32] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload.extend_from_slice(&24u16.to_be_bytes());
    payload.extend_from_slice(&1u16.to_be_bytes());
    payload.extend_from_slice(image);
    let mut resource = b"8BIM".to_vec();
    resource.extend_from_slice(&id.to_be_bytes());
    resource.extend_from_slice(&[0, 0]);
    resource.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    resource.extend_from_slice(&payload);
    if !payload.len().is_multiple_of(2) {
        resource.push(0);
    }
    let mut document = b"8BPS\0\x01".to_vec();
    document.extend_from_slice(&[0; 20]);
    document.extend_from_slice(&0u32.to_be_bytes());
    document.extend_from_slice(&(resource.len() as u32).to_be_bytes());
    document.extend_from_slice(&resource);
    document
}

fn red_tiff() -> Vec<u8> {
    let mut bytes = b"II\x2a\0\x08\0\0\0".to_vec();
    bytes.extend_from_slice(&10u16.to_le_bytes());
    for (tag, kind, count, value) in [
        (256u16, 4u16, 1u32, 1u32),
        (257, 4, 1, 1),
        (258, 3, 3, 134),
        (259, 3, 1, 1),
        (262, 3, 1, 2),
        (273, 4, 1, 140),
        (277, 3, 1, 3),
        (278, 4, 1, 1),
        (279, 4, 1, 3),
        (284, 3, 1, 1),
    ] {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[8, 0, 8, 0, 8, 0]);
    bytes.extend_from_slice(&[255, 0, 0]);
    bytes
}

fn red_pdf() -> Vec<u8> {
    let red = b"1 0 0 rg 0 0 200 100 re f\n";
    let blue = b"0 0 1 rg 0 0 200 100 re f\n";
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_vec(),
        format!("<< /Length {} >>\nstream\n", red.len())
            .into_bytes()
            .into_iter()
            .chain(red.iter().copied())
            .chain(b"endstream".iter().copied())
            .collect(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 6 0 R >>".to_vec(),
        format!("<< /Length {} >>\nstream\n", blue.len())
            .into_bytes()
            .into_iter()
            .chain(blue.iter().copied())
            .chain(b"endstream".iter().copied())
            .collect(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 7\n0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}
