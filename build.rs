use std::{env, fs, path::Path};

use lightningcss::stylesheet::{
    ParserOptions as CssParserOptions, PrinterOptions as CssPrinterOptions, StyleSheet,
};
use oxc::{
    allocator::Allocator as OxcAllocator,
    codegen::{Codegen as OxcCodegen, CodegenOptions as OxcCodegenOptions},
    minifier::{
        MangleOptions as OxcMangleOptions, Minifier as OxcMinifier,
        MinifierOptions as OxcMinifierOptions,
    },
    parser::Parser as OxcParser,
    span::SourceType as OxcSourceType,
};

fn main() {
    println!("cargo:rerun-if-changed=src/assets");

    let out_dir = env::var("OUT_DIR").unwrap();

    for asset in fs::read_dir("src/assets").unwrap() {
        let asset = asset.unwrap();

        let asset_path = asset.path();

        if asset_path.is_dir() {
            continue;
        }

        match asset_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
        {
            "css" => {
                let content = fs::read_to_string(&asset_path).unwrap();
                let filename = asset_path.file_name().unwrap().to_str().unwrap();

                let mut stylesheet = StyleSheet::parse(
                    &content,
                    CssParserOptions {
                        filename: filename.to_string(),
                        ..Default::default()
                    },
                )
                .unwrap();
                stylesheet.minify(Default::default()).unwrap();
                let minified_content = stylesheet
                    .to_css(CssPrinterOptions {
                        minify: true,
                        ..Default::default()
                    })
                    .unwrap();

                let output_path = Path::new(&out_dir).join(filename);
                fs::write(output_path, minified_content.code).unwrap();
            }

            "js" => {
                let content = fs::read_to_string(&asset_path).unwrap();

                let filename = asset_path.file_name().unwrap().to_str().unwrap();

                let allocator = OxcAllocator::default();
                let mut parse_return =
                    OxcParser::new(&allocator, &content, OxcSourceType::cjs()).parse();
                let minify_return = OxcMinifier::new(OxcMinifierOptions {
                    mangle: Some(OxcMangleOptions {
                        top_level: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .build(&allocator, &mut parse_return.program);
                let minified_content = OxcCodegen::new()
                    .with_options(OxcCodegenOptions::minify())
                    .with_scoping(minify_return.scoping)
                    .build(&parse_return.program)
                    .code;

                let output_path = Path::new(&out_dir).join(filename);
                fs::write(output_path, minified_content).unwrap();
            }

            _ => {}
        }
    }
}
