//! One operation/argument contract for the CLI and AXIS file-analysis forms.
use super::record::quote;

#[derive(Clone, Copy)]
pub enum Field {
    Object,
    NewObject,
    Setup,
    NewSetup,
    Label,
    Capture,
    SourceCapture,
    Design,
    NewDesign,
    Analysis,
    NewAnalysis,
    File,
    Request,
    Directory,
    NewDirectory,
    NewFile,
    Units,
}
impl Field {
    fn description(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Object => ("object", "Object", "object"),
            Self::NewObject => ("new-object", "New object ID", "text"),
            Self::Setup => ("setup", "Setup", "setup"),
            Self::NewSetup => ("new-setup", "New setup ID", "text"),
            Self::Label => ("label", "Label", "text"),
            Self::Capture => ("capture", "New capture ID", "text"),
            Self::SourceCapture => ("source-capture", "Retained capture", "capture"),
            Self::Design => ("design", "STL revision", "design"),
            Self::NewDesign => ("new-design", "New design revision ID", "text"),
            Self::Analysis => ("analysis", "Analysis", "analysis"),
            Self::NewAnalysis => ("new-analysis", "New analysis ID", "text"),
            Self::File => ("file", "Source file", "file"),
            Self::Request => ("request", "Analysis request file", "file"),
            Self::Directory => ("directory", "Directory", "directory"),
            Self::NewDirectory => ("new-directory", "New output directory", "new-path"),
            Self::NewFile => ("new-file", "New output file", "new-path"),
            Self::Units => ("units", "Millimetres per STL unit", "text"),
        }
    }
    fn json(self) -> String {
        let (key, label, kind) = self.description();
        format!(
            "{{\"key\":{},\"label\":{},\"kind\":{}}}",
            quote(key),
            quote(label),
            quote(kind)
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    List,
    Create,
    Show,
    AddSetup,
    Import,
    AttachDesign,
    ExportCaptures,
    Inspect,
    Prepare,
    PrepareStock,
    FitStock,
    PrepareSurface,
    FitSurface,
    Fit,
    ShowFit,
    ExportFit,
    Locate,
    Browse,
    LoadRequest,
    SaveRequest,
}
pub struct Spec {
    pub operation: Operation,
    pub name: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub fields: &'static [Field],
}
use Field::*;
use Operation as Op;
pub const OPERATIONS: &[Spec] = &[
    Spec { operation: Op::List, name: "list", label: "List objects", description: "Read the retained objects. Select Show object to inspect its setups, captures and design revisions.", fields: &[] },
    Spec { operation: Op::Create, name: "create", label: "Create object", description: "Create a new object record. Existing IDs are preserved.", fields: &[NewObject, Label] },
    Spec { operation: Op::Show, name: "show", label: "Show object", description: "Inspect its captures, failures, setup IDs, design revisions and analysis candidates.", fields: &[Object] },
    Spec { operation: Op::AddSetup, name: "add-setup", label: "Add setup", description: "Record a distinct placement of this object without changing a previous setup.", fields: &[Object, NewSetup, Label] },
    Spec { operation: Op::Import, name: "import-capture", label: "Import capture", description: "Retain an original probe ledger and its exact trigger records. Failed captures remain quarantined.", fields: &[Object, Setup, Capture, File] },
    Spec { operation: Op::AttachDesign, name: "attach-design", label: "Attach design", description: "Retain a named FCStd, STEP or STL revision. Positional fitting uses an STL with explicit units.", fields: &[Object, NewDesign, File] },
    Spec { operation: Op::ExportCaptures, name: "export-freecad", label: "Export captures", description: "Export retained measurements and design files into a new directory for inspection.", fields: &[Object, Setup, NewDirectory] },
    Spec { operation: Op::Inspect, name: "inspect-stl", label: "Inspect STL", description: "Read model dimensions and mesh checks using the specified unit conversion.", fields: &[File, Units] },
    Spec { operation: Op::Prepare, name: "prepare-fit", label: "Prepare fit request", description: "Create an editable request with retained fine-contact references. Fill REQUIRED values and assign fit/check/stock roles, then save a new request file.", fields: &[Object, Setup, Design] },
    Spec { operation: Op::PrepareStock, name: "prepare-stock", label: "Prepare stock outline", description: "Use the tracer's retained phase records to select ordered rim contacts and withhold the seam check. Fill calibration, fit scale and support limits; no reference STL is required.", fields: &[Object, Setup, SourceCapture] },
    Spec { operation: Op::FitStock, name: "fit-stock", label: "Estimate stock outline", description: "Fit the measured rim without a fixed stock shape. Retain original contacts, local residuals, independent checks and requests for further measurements. This does not certify stock volume or issue probing commands.", fields: &[Object, Setup, NewAnalysis, Request] },
    Spec { operation: Op::PrepareSurface, name: "prepare-stock-surface", label: "Prepare 3D stock surfaces", description: "Select retained fine contacts across this setup for local 3D surface fitting. Fill calibration and neighborhood settings, keep independent checks and exclude unrelated objects explicitly. A single rim line cannot determine wall slope.", fields: &[Object, Setup] },
    Spec { operation: Op::FitSurface, name: "fit-stock-surface", label: "Estimate 3D stock surfaces", description: "Estimate local surfaces and slope from retained 3D contacts without a nominal stock shape. Inspect residuals, unresolved regions and independent checks through Inspect analysis. Local patches leave unmeasured volume unknown.", fields: &[Object, Setup, NewAnalysis, Request] },
    Spec { operation: Op::LoadRequest, name: "load-request", label: "Open analysis request", description: "Open a request draft for editing. Save edits to a new file to preserve the original.", fields: &[Request] },
    Spec { operation: Op::SaveRequest, name: "save-request", label: "Save request as", description: "Save the current request editor to a new file. Existing files are never overwritten. CLI input is read from standard input.", fields: &[NewFile] },
    Spec { operation: Op::Fit, name: "fit", label: "Calculate placement", description: "Fit the selected contacts and retain residuals, independent checks and named stock-face dimensions. Numerical convergence remains an unreviewed placement proposal.", fields: &[Object, Setup, NewAnalysis, Request] },
    Spec { operation: Op::ShowFit, name: "show-fit", label: "Inspect analysis", description: "Read a retained analysis report, request and all residual rows. Incomplete publication remains an error.", fields: &[Object, Setup, Analysis] },
    Spec { operation: Op::ExportFit, name: "export-fit", label: "Export analysis", description: "Copy the analysis geometry, reports and retained sources into a new directory.", fields: &[Object, Setup, Analysis, NewDirectory] },
    Spec { operation: Op::Locate, name: "locate", label: "Map named locations", description: "Transform model-space points from a CSV to candidate machine coordinates in a new CSV. This does not apply offsets or generate machine instructions.", fields: &[Object, Setup, Analysis, File, NewFile] },
    Spec { operation: Op::Browse, name: "browse", label: "Browse files", description: "List a directory for selecting retained captures, designs, requests and output paths.", fields: &[Directory] },
];
impl Spec {
    pub fn result_kind(&self) -> &'static str {
        match self.operation {
            Op::Prepare | Op::PrepareStock | Op::PrepareSurface | Op::LoadRequest => "request",
            _ => "json",
        }
    }
    fn json(&self) -> String {
        format!("{{\"command\":{},\"label\":{},\"description\":{},\"fields\":[{}],\"result\":{},\"input\":{}}}", quote(self.name),quote(self.label),quote(self.description),self.fields.iter().map(|f| f.json()).collect::<Vec<_>>().join(","),quote(self.result_kind()),quote(if self.operation == Op::SaveRequest { "request" } else { "none" }))
    }
}
pub fn json(store: &std::path::Path) -> String {
    format!("{{\"schema\":\"dmc2.object-map-operations.v1\",\"store\":{},\"initial_command\":{},\"operations\":[{}]}}",quote(&store.display().to_string()),quote(OPERATIONS[0].name),OPERATIONS.iter().map(Spec::json).collect::<Vec<_>>().join(","))
}
pub fn usage() -> String {
    let mut result = String::from(
        "Usage: dmc2ctl object-map [--store DIRECTORY] COMMAND\n\nCommands:\n  catalog\n",
    );
    for spec in OPERATIONS {
        result.push_str(&format!(
            "  {}{}\n",
            spec.name,
            spec.fields
                .iter()
                .map(|f| format!(" {}", f.description().0.to_uppercase()))
                .collect::<String>()
        ));
    }
    result.push_str("\nIDs use lowercase letters, digits, hyphens and underscores. Quote labels containing spaces.\nThe default store is var/objects beneath the DMC2 project.\nThese commands operate on retained files only. They issue no machine commands.\nExisting records and outputs are never overwritten; choose a new ID or path to retry.\n");
    result
}
