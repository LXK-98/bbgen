//! ELK's layered layout, for the circuit view: symbols placed and wires routed by the Eclipse Layout Kernel.
//!
//! ELK is Java; elk-rs (https://github.com/openedges/elk-rs) is ELK ported to Rust, checked against Java ELK's
//! layouts, and lays a schematic out in milliseconds. Its layouts are elkjs's (ELK compiled to JavaScript, which bbgen
//! ran in QuickJS before) to within a few millionths of a pixel. Only the layered algorithm is registered: elk-rs's
//! own entry point registers all of ELK's, which doubled bbgen's size.
//!
//! A graph goes in and comes back in ELK's JSON format, with positions and wire routes filled in:
//! https://eclipse.dev/elk/documentation/tooldevelopers/graphdatastructure/jsonformat.html. Its options are ELK's
//! own (https://eclipse.dev/elk/reference.html). The same graph always gets the same layout.

use org_eclipse_elk_alg_layered::org::eclipse::elk::alg::layered::layered_layout_provider::LayeredLayoutProvider;
use org_eclipse_elk_alg_layered::org::eclipse::elk::alg::layered::options::LayeredMetaDataProvider;
use org_eclipse_elk_alg_layered::org::eclipse::elk::alg::layered::plain_java_initialization::initialize_plain_java_layout;
use org_eclipse_elk_core::org::eclipse::elk::core::abstract_layout_provider::AbstractLayoutProvider;
use org_eclipse_elk_core::org::eclipse::elk::core::data::LayoutMetaDataService;
use org_eclipse_elk_core::org::eclipse::elk::core::graph_layout_engine::IGraphLayoutEngine;
use org_eclipse_elk_core::org::eclipse::elk::core::recursive_graph_layout_engine::RecursiveGraphLayoutEngine;
use org_eclipse_elk_core::org::eclipse::elk::core::util::{AlgorithmFactory, BasicProgressMonitor, InstancePool, Maybe};
use org_eclipse_elk_graph_json::org::eclipse::elk::graph::json::{ElkGraphJson, JsonImporter};
use serde_json::Value;
use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

fn ready() {
    static READY: OnceLock<()> = OnceLock::new();
    READY.get_or_init(|| {
        initialize_plain_java_layout();
        let service = LayoutMetaDataService::get_instance();
        service.register_layout_meta_data_provider(&LayeredMetaDataProvider);
        let make = || Box::new(LayeredLayoutProvider::new()) as Box<dyn AbstractLayoutProvider>;
        service.override_algorithm_provider_pool("org.eclipse.elk.layered",
            Arc::new(InstancePool::new(Box::new(AlgorithmFactory::new(make)))));
    });
}

/// The graph laid out by ELK: the same graph, with each node's `x` and `y`, each port's, and each edge's `sections`
/// (its route: start, bend points, end) and `junctionPoints` filled in.
pub fn layout(graph: &Value) -> Result<Value, String> {
    ready();
    // ELK's exceptions are panics in elk-rs
    panic::catch_unwind(AssertUnwindSafe(|| {
        let shared = Rc::new(RefCell::new(graph.clone()));
        let mut importer: Maybe<JsonImporter> = Maybe::default();
        let root = ElkGraphJson::for_graph_shared(shared.clone()).remember_importer(&mut importer).lenient(false)
            .to_elk().map_err(|e| e.to_string())?;
        RecursiveGraphLayoutEngine::new().layout(&root, &mut BasicProgressMonitor::new());
        importer.get_mut().ok_or("no importer")?.transfer_layout(&root).map_err(|e| e.to_string())?;
        let out = shared.borrow().clone();
        Ok(out)
    }))
    .unwrap_or_else(|p| Err(p.downcast_ref::<String>().cloned()
        .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "it failed".into())))
    .map_err(|e: String| format!("ELK: {}", e))
}
