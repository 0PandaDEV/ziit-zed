use zed_extension_api as zed;

struct ZiitExtension {

}

impl zed::Extension for ZiitExtension {
    fn new() -> Self {
        Self {

        }
    }
}

zed::register_extension!(ZiitExtension);
