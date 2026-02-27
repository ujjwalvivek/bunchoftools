use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use bunch_core::{AssetBinding, ExecutionBindings, ExecutorKind, Recipe};
use bunch_native::{NativeRunner, load_recipe, native_capabilities, probe_image};
use bunch_planner::{ExecutionPlan, Planner};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    let Some(command) = arguments.first().and_then(|value| value.to_str()) else {
        print_help();
        return Ok(());
    };
    match command {
        "sample" => print_json(&Recipe::image_resize()),
        "foundation-sample" => print_json(&Recipe::foundation_demo()),
        "capabilities" => print_json(native_capabilities().all()),
        "probe" => {
            let path = required_path(&arguments, 1, "probe requires INPUT")?;
            print_json(&probe_image(&path)?)
        }
        "image" => {
            let path = required_path(&arguments, 1, "image requires INPUT")?;
            run_plan(plan_for_path(&Recipe::image_resize(), &path)?)
        }
        "validate" => {
            let path = required_path(&arguments, 1, "validate requires RECIPE.json")?;
            let recipe = load_recipe(&path)?;
            println!("valid: {} (schema {})", recipe.name, recipe.schema_version);
            Ok(())
        }
        "plan" => {
            let plan = plan_from_arguments(&arguments)?;
            print_json(&plan)
        }
        "run" => run_plan(plan_from_arguments(&arguments)?),
        "demo" => run_plan(plan_demo()?),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command {other}")),
    }
}

fn plan_from_arguments(arguments: &[std::ffi::OsString]) -> Result<ExecutionPlan, String> {
    let recipe_path = required_path(arguments, 1, "plan/run requires RECIPE.json INPUT")?;
    let input_path = required_path(arguments, 2, "plan/run requires RECIPE.json INPUT")?;
    let recipe = load_recipe(&recipe_path)?;
    plan_for_path(&recipe, &input_path)
}

fn plan_demo() -> Result<ExecutionPlan, String> {
    plan_for_path(
        &Recipe::foundation_demo(),
        Path::new("foundation-demo.input"),
    )
}

fn plan_for_path(recipe: &Recipe, input_path: &Path) -> Result<ExecutionPlan, String> {
    let input_slot = recipe
        .inputs
        .first()
        .ok_or_else(|| "recipe has no input slot".to_owned())?;
    let display_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("input")
        .to_owned();
    let output_directory = input_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_string_lossy()
        .into_owned();
    let bindings = ExecutionBindings {
        inputs: BTreeMap::from([(
            input_slot.id.clone(),
            AssetBinding {
                locator: input_path.to_string_lossy().into_owned(),
                display_name,
            },
        )]),
        output_directory,
    };
    Planner
        .plan(
            recipe,
            bindings,
            &native_capabilities(),
            ExecutorKind::Native,
        )
        .map_err(|error| error.to_string())
}

fn run_plan(plan: ExecutionPlan) -> Result<(), String> {
    let handle = NativeRunner::new()
        .submit(plan)
        .map_err(|error| error.to_string())?;
    let events = handle.events();
    while let Ok(event) = events.recv_blocking() {
        println!(
            "{}",
            serde_json::to_string(&event).map_err(|error| error.to_string())?
        );
        if event.terminal_state().is_some() {
            break;
        }
    }
    Ok(())
}

fn required_path(
    arguments: &[std::ffi::OsString],
    index: usize,
    message: &str,
) -> Result<PathBuf, String> {
    arguments
        .get(index)
        .map(PathBuf::from)
        .ok_or_else(|| message.to_owned())
}

fn print_json(value: &(impl serde::Serialize + ?Sized)) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn print_help() {
    println!(
        "Bunch of Tools M2\n\n\
         bunch-cli sample\n\
         bunch-cli foundation-sample\n\
         bunch-cli capabilities\n\
         bunch-cli probe INPUT\n\
         bunch-cli image INPUT\n\
         bunch-cli validate RECIPE.json\n\
         bunch-cli plan RECIPE.json INPUT\n\
         bunch-cli run RECIPE.json INPUT\n\
         bunch-cli demo"
    );
}
