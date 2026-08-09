#[cfg(target_arch = "wasm32")]
use js_sys::ArrayBuffer;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;
#[cfg(target_arch = "wasm32")]
use web_sys::Response;

pub struct FileReader {}

impl FileReader {
    pub async fn read_file(path: &str) -> Vec<u8> {
        #[cfg(target_arch = "wasm32")]
        return FileReader::read_file_wasm(path).await;

        #[cfg(not(target_arch = "wasm32"))]
        return FileReader::read_file_native(path).await;
    }

    #[cfg(target_arch = "wasm32")]
    async fn read_file_wasm(path: &str) -> Vec<u8> {
        let window = web_sys::window().expect("Failed to create reference to window");
        let response_js = JsFuture::from(window.fetch_with_str(path))
            .await
            .expect("Failed to fetch file");
        assert!(response_js.is_instance_of::<Response>());
        let response: Response = response_js.dyn_into().unwrap();

        let buffer_js = JsFuture::from(
            response
                .array_buffer()
                .expect("Failed to read array buffer"),
        )
        .await
        .expect("Could not read response body buffer");
        assert!(buffer_js.is_instance_of::<ArrayBuffer>());
        let buffer: ArrayBuffer = buffer_js.dyn_into().unwrap();
        let u8_buffer: js_sys::Uint8Array = js_sys::Uint8Array::new(&buffer);
        let mut buff_vec = vec![0; u8_buffer.length() as usize];
        u8_buffer.copy_to(&mut buff_vec[..]);
        buff_vec
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn read_file_native(path: &str) -> Vec<u8> {
        use std::env;
        use std::fs::File;
        use std::io::Read;

        let mut total_path = env::current_dir().unwrap();
        total_path.push(path);
        println!("reading file {:?}", total_path);
        let mut file = File::open(total_path).expect("Failed to open file");
        let mut buff_vec = Vec::<u8>::new();
        file.read_to_end(&mut buff_vec)
            .expect("Could not read bytes from file");
        buff_vec
    }
}
