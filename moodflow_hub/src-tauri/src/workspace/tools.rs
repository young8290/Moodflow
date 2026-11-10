//! Explicit AI text actions. No history, partial persistence, automatic delivery, fallback or retry.
use super::*;
use moodflow_hub_core::infra::templates::load_prompt;
use moodflow_hub_gateway::{InlineImage, reference::Source};
use zeroize::Zeroizing;

fn input(params: &Value) -> Result<(&str, &str, &str, &str)> {
    let action = string(params, "action")?;
    ensure!(
        [
            "generate",
            "rewrite",
            "answer",
            "translate",
            "poem",
            "image_prompt",
            "latex",
            "reference"
        ]
        .contains(&action),
        "ai_tools.invalid"
    );
    let text = string(params, "text")?;
    let instructions = params["instructions"].as_str().unwrap_or("");
    let language = params["language"].as_str().unwrap_or("");
    ensure!(
        text.encode_utf16().count() <= 12000
            && instructions.encode_utf16().count() <= 1024
            && language.encode_utf16().count() <= 64,
        "ai_tools.limit"
    );
    ensure!(
        !text.trim().is_empty()
            || (action == "latex" && params["images"].as_array().is_some_and(|v| !v.is_empty())),
        "ai_tools.empty"
    );
    Ok((action, text, instructions, language))
}
fn images(
    app: &AppHandle,
    paths: &[Value],
    cancel: Arc<AtomicBool>,
) -> Result<(Vec<InlineImage>, String)> {
    ensure!(paths.len() <= 3, "ai_tools.image_limit");
    let root = crate::paths::tool_data_dir()?.join("capture");
    crate::private_fs::ensure(&root)?;
    let directory = tempfile::Builder::new()
        .prefix("capture-work-ai-")
        .tempdir_in(root)?;
    let mut images = Vec::new();
    let mut text = String::new();
    for (i, value) in paths.iter().enumerate() {
        ensure!(!cancel.load(Ordering::Acquire), "ai_tools.cancelled");
        let path = PathBuf::from(
            value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("ai_tools.invalid"))?,
        );
        let metadata = fs::symlink_metadata(&path)?;
        ensure!(
            regular_file(&metadata) && metadata.len() <= 4 * 1024 * 1024,
            "ai_tools.image_limit"
        );
        let mut bytes = Zeroizing::new(Vec::new());
        fs::File::open(&path)?
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 4 * 1024 * 1024, "ai_tools.image_limit");
        InlineImage::from_bytes(&bytes).map_err(|_| anyhow::anyhow!("ai_tools.image_failed"))?;
        let snapshot = directory.path().join(format!("image-{i}.bin"));
        fs::write(&snapshot, &bytes)?;
        let ocr = crate::capture::tool_ocr(app, &snapshot, cancel.clone())
            .map_err(|_| anyhow::anyhow!("ai_tools.image_failed"))?;
        text.push('\n');
        text.push_str(&ocr);
        ensure!(text.encode_utf16().count() <= 32000, "ai_tools.limit");
        let mut normalized = Zeroizing::new(Vec::new());
        fs::File::open(snapshot.with_extension("png"))?
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut normalized)?;
        images.push(
            InlineImage::from_bytes(&normalized)
                .map_err(|_| anyhow::anyhow!("ai_tools.image_limit"))?,
        );
    }
    Ok((images, text))
}
impl Runtime {
    pub fn cancel_tool(&self, id: &str) {
        if let Some(cancel) = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
        {
            cancel.store(true, Ordering::Release);
        }
    }
    pub async fn tool(self: &Arc<Self>, app: AppHandle, params: Value) -> Result<Value> {
        let (action, text, instructions, language) = input(&params)?;
        ensure!(
            params["consent"].as_bool() == Some(true),
            "ai_tools.consent"
        );
        ensure!(allowed_async(app.clone()).await, "ai_tools.privacy");
        let id = string(&params, "id")?.to_owned();
        moodflow_mailbox::valid_id(&id)?;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
            ensure!(
                !running.contains_key(&id) && running.len() < 4,
                "ai_tools.busy"
            );
            running.insert(id.clone(), cancel.clone());
        }
        let _running = Running {
            runtime: self.clone(),
            id,
        };
        let epoch = self.epoch();
        let (lex, banned) = self
            .safety
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ai_tools.safety_unavailable"))?;
        let (version, prompt) =
            load_prompt(&crate::paths::hub_template_dirs()?, "prompts/tools.md", &[])
                .map_err(|_| anyhow::anyhow!("ai_tools.safety_unavailable"))?;
        let ai = app.state::<Arc<crate::gateway::Ai>>().inner().clone();
        let (source, masked) = ai.view();
        let side = masked
            .llm
            .ok_or_else(|| anyhow::anyhow!("ai_tools.ai_unavailable"))?;
        ensure!(
            source != crate::gateway::Source::None,
            "ai_tools.ai_unavailable"
        );
        let model = string(&params, "model")?;
        ensure!(
            side.base_url == string(&params, "endpoint")? && side.models.iter().any(|m| m == model),
            "ai_tools.model_changed"
        );
        let gateway = ai.gateway();
        // Check the user's entire explicit request before an external search or image interpretation.
        let user_text = format!("{text}\n{instructions}\n{language}");
        if chat::lexicon_hit(&user_text, lex, SafeMode::Off) {
            show_safety(&app);
            anyhow::bail!("ai_tools.safety");
        }
        let mut sources: Vec<Source> = vec![];
        if action == "reference" {
            ensure!(
                params["search_consent"].as_bool() == Some(true),
                "ai_tools.search_consent"
            );
            ensure!(text.encode_utf16().count() <= 512, "ai_tools.query_limit");
            let query = text.to_owned();
            sources = self
                .guarded(&app, epoch, &cancel, async move {
                    moodflow_hub_gateway::reference::search(&query)
                        .await
                        .map_err(|_| AiError::InvalidOutput)
                })
                .await
                .map_err(|_| anyhow::anyhow!("ai_tools.search_failed"))?;
            ensure!(!sources.is_empty(), "ai_tools.no_sources");
        }
        let paths = params["images"].as_array().cloned().unwrap_or_default();
        ensure!(paths.is_empty() || action == "latex", "ai_tools.invalid");
        ensure!(paths.len() <= 3, "ai_tools.image_limit");
        let (pictures, ocr) = if paths.is_empty() {
            (vec![], String::new())
        } else {
            let handle = app.clone();
            let stop = cancel.clone();
            tauri::async_runtime::spawn_blocking(move || images(&handle, &paths, stop)).await??
        };
        let data = json!({"action":action,"input":text,"instructions":instructions,"language":language,"sources":sources.iter().enumerate().map(|(i,s)|json!({"number":i+1,"title":s.reference.title,"doi":s.reference.doi,"abstract":s.abstract_text})).collect::<Vec<_>>()});
        let messages = vec![
            AiMessage {
                role: "system".into(),
                content: prompt,
            },
            AiMessage {
                role: "user".into(),
                content: data.to_string(),
            },
        ];
        let checked = messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            + &ocr;
        ensure!(checked.encode_utf16().count() <= 50000, "ai_tools.limit");
        if chat::lexicon_hit(&checked, lex, SafeMode::Off) {
            show_safety(&app);
            anyhow::bail!("ai_tools.safety");
        }
        let verdict = self
            .guarded(
                &app,
                epoch,
                &cancel,
                ai.judge(JudgeRequest {
                    questions: vec![Question::Crisis],
                    state: [("message".into(), json!(checked))].into_iter().collect(),
                }),
            )
            .await;
        if verdict.as_ref().is_ok_and(|v|matches!(v.answers.get(&Question::Crisis),Some(Answer::Noul{p}) if *p>=chat::JEV_CRISIS_THRESHOLD)) {show_safety(&app);anyhow::bail!("ai_tools.safety");}
        ensure!(
            !cancel.load(Ordering::Acquire)
                && self.epoch() == epoch
                && allowed_async(app.clone()).await,
            "ai_tools.cancelled"
        );
        let reply = self
            .guarded(
                &app,
                epoch,
                &cancel,
                gateway.complete_tool(
                    CompleteRequest {
                        scenario: if action == "rewrite" {
                            Scenario::Rewrite
                        } else {
                            Scenario::Chat
                        },
                        prompt_ver: format!("tools/{version}"),
                        messages,
                    },
                    model,
                    &pictures,
                ),
            )
            .await
            .map_err(|_| anyhow::anyhow!("ai_tools.ai_failed"))?;
        let mut body = reply.text.trim().to_owned();
        if action == "latex" {
            body = latex_body(&body)?;
        }
        if action == "reference" {
            let style = params["style"].as_str().unwrap_or("gbt-compatible");
            let references = sources
                .iter()
                .map(|s| {
                    moodflow_cite::format(&s.reference, style)
                        .map(|text| format!("{text} DOI: {}", s.reference.doi))
                })
                .collect::<Result<Vec<_>>>()?;
            body = moodflow_cite::cited_text(&body, &references)
                .map_err(|_| anyhow::anyhow!("ai_tools.invalid_citations"))?;
        }
        match chat::check_reply(&body, banned, lex) {
            ReplyCheck::Ok(_) => {}
            ReplyCheck::Banned => anyhow::bail!("ai_tools.invalid_output"),
            ReplyCheck::Unsafe => {
                show_safety(&app);
                anyhow::bail!("ai_tools.safety");
            }
        }
        ensure!(body.encode_utf16().count() <= 16384, "ai_tools.limit");
        let (_, latest) = ai.view();
        ensure!(latest.llm.as_ref().is_some_and(|s|s.base_url==side.base_url && s.models.iter().any(|m|m==model)),"ai_tools.model_changed");
        ensure!(
            !cancel.load(Ordering::Acquire)
                && self.epoch() == epoch
                && allowed_async(app.clone()).await,
            "ai_tools.cancelled"
        );
        Ok(
            json!({"text":body,"sources":sources,"model":reply.model,"mock":source==crate::gateway::Source::DevMock}),
        )
    }
}
fn latex_body(value: &str) -> Result<String> {
    let text = value.trim();
    let text = if let Some(rest) = text
        .strip_prefix("```latex\n")
        .or_else(|| text.strip_prefix("```tex\n"))
        .or_else(|| text.strip_prefix("```\n"))
    {
        rest.strip_suffix("```")
            .ok_or_else(|| anyhow::anyhow!("ai_tools.invalid_output"))?
            .trim()
    } else {
        text
    };
    ensure!(
        !text.is_empty()
            && !text.contains("```")
            && !text.split('<').skip(1).any(|tail| tail
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '!' || c == '/')
                && tail.contains('>'))
            && !text.contains("\\input")
            && !text.contains("\\include")
            && !text.contains("\\write")
            && !text.contains("\\def")
            && !text.contains("\\href")
            && !text.contains("\\url")
            && !text.contains("\\html"),
        "ai_tools.invalid_output"
    );
    Ok(text.to_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn factory_tool_instructions_do_not_trigger_the_input_crisis_gate() {
        let dirs = moodflow_hub_core::infra::templates::TemplateDirs::factory_only(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../hub_templates"),
        );
        let (_, prompt) = load_prompt(&dirs, "prompts/tools.md", &[]).unwrap();
        let lex = CrisisLexicon::load(&dirs).unwrap();
        assert!(!chat::lexicon_hit(
            &(prompt.clone() + "\n请改写这一段合成测试文字。"),
            &lex,
            SafeMode::Off
        ));
        assert!(chat::lexicon_hit(
            &(prompt + "\n伤害自己"),
            &lex,
            SafeMode::Off
        ));
        assert_eq!(latex_body("a < b").unwrap(), "a < b");
    }
    #[test]
    fn bounded_actions_and_untrusted_latex() {
        assert!(input(&json!({"action":"run_terminal","text":"x"})).is_err());
        assert!(input(&json!({"action":"generate","text":"😀".repeat(6001)})).is_err());
        assert_eq!(
            latex_body("```latex\n\\alpha + \\beta\n```").unwrap(),
            r"\alpha + \beta"
        );
        for value in [
            r"\input{file}",
            "<img src=x>",
            "```latex\nx",
            r"\includegraphics{https://x}",
        ] {
            assert!(latex_body(value).is_err());
        }
    }
}
