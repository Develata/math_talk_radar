use super::evidence::Result;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct TestId {
    pub binary: String,
    pub test: String,
}

pub fn inventory(value: &Value, modules: &BTreeSet<String>) -> Result<BTreeSet<TestId>> {
    let mut tests = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for (binary, suite) in value["rust-suites"]
        .as_object()
        .ok_or("nextest suites missing")?
    {
        let module = suite["package-name"]
            .as_str()
            .ok_or("nextest package missing")?;
        if !modules.contains(module) || suite["status"] != "listed" {
            return Err(format!("unexpected/unlisted test suite: {binary}"));
        }
        for (name, test) in suite["testcases"].as_object().ok_or("testcases missing")? {
            if test["ignored"] != false
                || test["filter-match"]["status"] != "matches"
                || test["kind"] != "test"
            {
                return Err(format!(
                    "ignored/filtered/unrecognized required test: {binary}::{name}"
                ));
            }
            covered.insert(module.to_owned());
            tests.insert(TestId {
                binary: binary.clone(),
                test: name.clone(),
            });
        }
    }
    if tests.is_empty()
        || &covered != modules
        || value["test-count"].as_u64() != Some(tests.len() as u64)
    {
        return Err("empty or incomplete module test inventory".into());
    }
    Ok(tests)
}

fn attributes(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
) -> Result<BTreeMap<String, String>> {
    element
        .attributes()
        .map(|attribute| {
            let attribute = attribute.map_err(|e| e.to_string())?;
            Ok((
                String::from_utf8(attribute.key.as_ref().to_vec()).map_err(|e| e.to_string())?,
                attribute
                    .decoded_and_normalized_value(
                        quick_xml::XmlVersion::Implicit1_0,
                        reader.decoder(),
                    )
                    .map_err(|e| e.to_string())?
                    .into_owned(),
            ))
        })
        .collect()
}

pub fn results(xml: &str) -> Result<BTreeMap<TestId, String>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut results = BTreeMap::new();
    let mut current: Option<(TestId, String)> = None;
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(element) | Event::Empty(element)
                if element.name().as_ref() == b"testcase" =>
            {
                if current.is_some() {
                    return Err("nested testcases".into());
                }
                let attrs = attributes(&reader, &element)?;
                let id = TestId {
                    binary: attrs
                        .get("classname")
                        .ok_or("JUnit classname missing")?
                        .clone(),
                    test: attrs.get("name").ok_or("JUnit test name missing")?.clone(),
                };
                // quick-xml expands empty elements below, so both styles share
                // the same closing-event handling.
                current = Some((id, "pass".into()));
            }
            Event::Start(element) | Event::Empty(element)
                if [
                    b"failure".as_slice(),
                    b"error",
                    b"skipped",
                    b"flakyFailure",
                    b"flakyError",
                    b"rerunFailure",
                    b"rerunError",
                ]
                .contains(&element.name().as_ref()) =>
            {
                if let Some((_, status)) = &mut current {
                    *status = "fail".into();
                } else {
                    return Err("JUnit failure outside testcase".into());
                }
            }
            Event::End(element) if element.name().as_ref() == b"testcase" => {
                let (id, status) = current.take().ok_or("unexpected testcase end")?;
                if results.insert(id, status).is_some() {
                    return Err("duplicate JUnit test result".into());
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if current.is_some() || results.is_empty() {
        return Err("empty/truncated JUnit results".into());
    }
    Ok(results)
}

pub fn verify(expected: &BTreeSet<TestId>, actual: &BTreeMap<TestId, String>) -> Result<()> {
    if expected.is_empty()
        || expected != &actual.keys().cloned().collect()
        || actual.values().any(|status| status != "pass")
    {
        return Err("test results contain missing, unexpected, skipped or failed tests".into());
    }
    Ok(())
}
