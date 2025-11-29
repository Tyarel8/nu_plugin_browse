use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt;
use nu_plugin::{
    EngineInterface, EvaluatedCall, MsgPackSerializer, Plugin, SimplePluginCommand, serve_plugin,
};
use nu_protocol::{Category, Example, LabeledError, Signature, SyntaxShape, Value};
use std::error::Error;

#[derive(Clone)]
struct HttpBrowse;

impl Plugin for HttpBrowse {
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    fn commands(&self) -> Vec<Box<dyn nu_plugin::PluginCommand<Plugin = Self>>> {
        vec![Box::new(HttpBrowse)]
    }
}

impl SimplePluginCommand for HttpBrowse {
    type Plugin = HttpBrowse;
    fn name(&self) -> &str {
        "http browse"
    }

    fn signature(&self) -> Signature {
        Signature::build("http browse")
            .required("url", SyntaxShape::String, "The URL to browse")
            .switch("no-stealth", "Disable stealth mode", None)
            .switch("with-head", "Disable headless mode", None)
            .category(Category::Network)
    }

    fn description(&self) -> &str {
        "Fetch an HTML page using a headless browser."
    }

    fn extra_description(&self) -> &str {
        "For this to work chrome/chromium has to be installed in the system."
    }

    fn examples(&'_ self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Fetch a page and output HTML",
            example: "http browse https://example.com",
            result: None,
        }]
    }

    fn run(
        &self,
        _plugin: &HttpBrowse,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let url: String = call.req(0)?;
        let disable_stealth = call.has_flag("no-stealth")?;
        let disable_headless = call.has_flag("with-head")?;

        match browse_page(&url, !disable_stealth, disable_headless) {
            Ok(html) => Ok(Value::string(html, call.head)),
            Err(e) => Err(LabeledError::new(format!("{e}")).with_label("browse failed", call.head)),
        }
    }
}

fn browse_page(url: &str, stealth: bool, disable_headless: bool) -> Result<String, Box<dyn Error>> {
    tokio::runtime::Runtime::new()?.block_on(async {
        let mut browser_config = BrowserConfig::builder().port(0);
        if disable_headless {
            browser_config = browser_config.with_head()
        };

        let (mut browser, mut handler) = Browser::launch(browser_config.build()?).await?;

        let _task = tokio::spawn(async move { while let Some(_event) = handler.next().await {} });

        let page = browser.new_page(url).await?;

        if stealth {
            page.enable_stealth_mode().await?;
        }

        page.evaluate(
            r#"() =>
  new Promise((resolve) => {
    let activeRequests = 0;
    let idleTimer;

    const done = (label) => {
      clearTimeout(idleTimer);
      idleTimer = setTimeout(() => resolve(`${label}-network-idle`), 500);
    };

    const origOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function (...args) {
      this.addEventListener('loadstart', () => {
        activeRequests++;
        clearTimeout(idleTimer);
      });
      this.addEventListener('loadend', () => {
        activeRequests--;
        if (activeRequests <= 0) done('xhr');
      });
      origOpen.apply(this, args);
    };

    const origFetch = window.fetch;
    window.fetch = async function (...args) {
      activeRequests++;
      clearTimeout(idleTimer);
      try {
        const response = await origFetch.apply(this, args);
        return response;
      } finally {
        activeRequests--;
        if (activeRequests <= 0) done('fetch');
      }
    };

    const maybeResolveImmediately = () => {
      if (document.readyState === 'complete' && activeRequests === 0) {
        done('initial');
      } else {
        window.addEventListener('load', () => done('load'), { once: true });
      }
    };

    maybeResolveImmediately();
  })"#,
        )
        .await?;

        let html = page.content().await?;
        browser.close().await?;

        Ok(html)
    })
}

fn main() {
    serve_plugin(&HttpBrowse, MsgPackSerializer)
}
