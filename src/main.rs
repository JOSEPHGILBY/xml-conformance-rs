use std::fmt::Display;

use clap::Parser;
use quick_xml::events::Event;
use tempfile::Builder;
use inquire::{MultiSelect};
use std::fs::{File, OpenOptions};
use std::io::{copy, Cursor, BufReader};
use std::{thread, time};
use std::env;
use std::path::{PathBuf, Path};
use serde::Deserialize;

use quick_xml::de::from_reader;
use quick_xml::reader::NsReader;

#[derive(Parser)]
struct Cli {
    /// The pattern to look for
    pattern: String,
    /// The path to the file to read
    path: std::path::PathBuf,
}

#[derive(Clone)]
struct ConformanceTestRelease<'a> {
    release_date: &'a str,
    download_zip_url: &'a str,
    filename: &'a str,
    sun_valid_tests_conf: Option<&'a str>,
    sun_invalid_tests_conf: Option<&'a str>,
    sun_non_wf_tests_conf: Option<&'a str>,
    sun_error_tests_conf: Option<&'a str>,
    ibm_valid_tests_conf: Option<&'a str>,
}

impl Display for ConformanceTestRelease<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.release_date)
    }
}

// Without xsl + dtd support, we have to hardcode the info contained :(
// in these tests. If we had xsl + dtd support, we could use the best xml
// parser on the rust market to download and parse the test cases and finally
// run the tests on the xml parser itself (in addition to other xml parsers)!
const RELEASES: [ConformanceTestRelease<'static>; 4] = [
    ConformanceTestRelease { 
        release_date: "2003-12-10", 
        download_zip_url: "https://www.w3.org/XML/Test/xmlts20031210.zip", 
        filename: "xmlts20031210.zip",
        sun_valid_tests_conf: Some("sun/sun-valid.xml"),
        sun_invalid_tests_conf: Some("sun/sun-invalid.xml"),
        sun_non_wf_tests_conf: Some("sun/sun-not-wf.xml"),
        sun_error_tests_conf: Some("sun/sun-error.xml"),
        ibm_valid_tests_conf: Some("ibm/ibm_oasis_valid.xml"),
    },
    ConformanceTestRelease { 
        release_date: "2008-02-05", 
        download_zip_url: "https://www.w3.org/XML/Test/xmlts20080205.zip", 
        filename: "xmlts20080205.zip",
        sun_valid_tests_conf: None,
        sun_invalid_tests_conf: None,
        sun_non_wf_tests_conf: None,
        sun_error_tests_conf: None,
        ibm_valid_tests_conf: None,
    },
    ConformanceTestRelease { 
        release_date: "2008-08-27", 
        download_zip_url: "https://www.w3.org/XML/Test/xmlts20080827.zip", 
        filename: "xmlts20080827.zip",
        sun_valid_tests_conf: None,
        sun_invalid_tests_conf: None,
        sun_non_wf_tests_conf: None,
        sun_error_tests_conf: None,
        ibm_valid_tests_conf: None,
    },
    ConformanceTestRelease { 
        release_date: "2013-09-23", 
        download_zip_url: "https://www.w3.org/XML/Test/xmlts20130923.zip", 
        filename: "xmlts20130923.zip",
        sun_valid_tests_conf: None,
        sun_invalid_tests_conf: None,
        sun_non_wf_tests_conf: None,
        sun_error_tests_conf: None,
        ibm_valid_tests_conf: None,
    }

];

#[derive(Debug, Deserialize)]
#[serde(rename = "TESTCASES")]
struct TestCasesTier1 {
    #[serde(rename = "@PROFILE")]
    profile: String,
    #[serde(rename = "@xml:base")]
    base: Option<String>,
    #[serde(rename = "TEST")]
    tests: Option<Vec<TestCase>>,
    #[serde(rename = "TESTCASES")]
    tier_2: Vec<TestCasesTier2>,
}

#[derive(Debug, Deserialize)]
#[serde(rename = "TESTCASES")]
struct TestCasesTier2 {
    #[serde(rename = "@PROFILE")]
    profile: String,
    #[serde(rename = "@xml:base")]
    base: Option<String>,
    #[serde(rename = "TEST")]
    tests: Vec<TestCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename = "TEST")]
struct TestCase {
    #[serde(rename = "@URI")]
    uri: String,
    #[serde(rename = "@ID")]
    id: String,
    #[serde(rename = "@ENTITIES")]
    entities: Option<String>,
    #[serde(rename = "@SECTIONS")]
    sections: String,
    #[serde(rename = "@TYPE")]
    expected_outcome: TestCaseType,
    #[serde(rename = "@OUTPUT")]
    output: Option<String>,
    #[serde(rename = "$text")]
    test_comment: String,
}
#[derive(PartialEq, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum TestCaseType {
    Valid,
    Invalid,
    NotWf,
    Error,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    //let args = Cli::parse();
    let selected_releases = MultiSelect::new("Select which test suites to run:", RELEASES.to_vec()).prompt()?;
    
    //let tmp_dir = Builder::new().prefix("example").tempdir()?;
    let curr_dir = std::env::current_dir()?;
    let curr_dir_path = curr_dir.as_path();
    for release in selected_releases.iter() {
        let zip_file_path = curr_dir_path.join(release.filename);
        if !zip_file_path.try_exists()? {
            let response = reqwest::get(release.download_zip_url).await?;
            let mut zip_file_write = File::create(&zip_file_path)?;
            let mut content = Cursor::new(response.bytes().await?);
            copy(&mut content, &mut zip_file_write)?;
        }

        let zip_file_parent = zip_file_path.parent().ok_or(format!("no parent for {:?}", zip_file_path.to_str()))?;
        let extract_dir_name = zip_file_path.file_stem().ok_or(format!("no file stem for {:?}", zip_file_path.to_str()))?;
        let extract_dir_path = zip_file_parent.join(extract_dir_name);

        if !extract_dir_path.try_exists()? {
            let zip_file_read = File::open(&zip_file_path)?;
            let mut archive = zip::ZipArchive::new(zip_file_read)?;
            archive.extract(&extract_dir_path)?;
        }
        let release_root_path = extract_dir_path.as_path().join("xmlconf");
        if let Some(conf_sub_path) = release.sun_valid_tests_conf {
            run_sun_tests_for(conf_sub_path, &release_root_path)?;
        }
        if let Some(conf_sub_path) = release.sun_invalid_tests_conf {
            run_sun_tests_for(conf_sub_path, &release_root_path)?;
        }
        if let Some(conf_sub_path) = release.sun_non_wf_tests_conf {
            run_sun_tests_for(conf_sub_path, &release_root_path)?;
        }
        if let Some(conf_sub_path) = release.sun_error_tests_conf {
            run_sun_tests_for(conf_sub_path, &release_root_path)?;
        }
        if let Some(conf_sub_path) = release.ibm_valid_tests_conf {
            run_ibm_tests_for(conf_sub_path, &release_root_path)?;
        }
    }
    
    Ok(())
}

fn run_ibm_tests_for(conf_sub_path: &str, release_root_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let (conf_file_parent_dir, reader) = setup_config_file_buf_reader(release_root_path, conf_sub_path)?;
    let tier_1: TestCasesTier1 = from_reader(reader)?;
    for test_cases in tier_1.tier_2.iter() {
        run_test_case_node(&test_cases.tests, &conf_file_parent_dir)?;
    }
    Ok(())
}

fn run_sun_tests_for(conf_sub_path: &str, release_root_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let (conf_file_parent_dir, reader) = setup_config_file_buf_reader(release_root_path, conf_sub_path)?;
    let test_cases: Vec<TestCase> = from_reader(reader)?;
    run_test_case_node(&test_cases, &conf_file_parent_dir)?;
    Ok(())
}

fn run_test_case_node(test_cases: &[TestCase], conf_file_parent_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for test_case in test_cases.iter() {
        let file_to_read_path = conf_file_parent_dir.join(test_case.uri.clone());
        let mut test_case_reader = NsReader::from_file(file_to_read_path.as_path())?;
        let mut buf = Vec::new();
        loop {
            let result = test_case_reader.read_resolved_event_into(&mut buf);
            match result { 
                Err(err) => {
                    if test_case.expected_outcome == TestCaseType::Valid || test_case.expected_outcome == TestCaseType::Invalid {
                        println!("------------------\nMISMATCHED OUTCOME\nGot error: {:?}\nIn well formed test: {:?}", err, test_case);
                    }      
                    break;
                },
                Ok((_, Event::Eof)) => {
                    if test_case.expected_outcome == TestCaseType::NotWf || test_case.expected_outcome == TestCaseType::Error {
                        println!("------------------\nMISMATCHED OUTCOME\nParsed non-well formed document\nFor test: {:?}", test_case);
                    }
                    break
                },
                _ => {}
            }
        }
    }
    Ok(())
}

fn setup_config_file_buf_reader(release_root_path: &Path, conf_sub_path: &str) -> Result<(PathBuf, BufReader<File>), Box<dyn std::error::Error>> {
    let conf_path = release_root_path.join(conf_sub_path);
    let conf_file_read = File::open(&conf_path)?;
    let conf_file_parent_dir = conf_path.parent().ok_or(format!("no parent for {:?}", conf_path.to_str()))?.to_path_buf();
    let reader = BufReader::new(conf_file_read);
    Ok((conf_file_parent_dir, reader))
}
