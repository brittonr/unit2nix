#include <nix/expr/eval.hh>
#include <nix/expr/primops.hh>
#include <nix/expr/value.hh>
#include <nix/expr/json-to-value.hh>
#include <nix/expr/value-to-json.hh>
#include <nlohmann/json.hpp>

// Rust FFI declarations
extern "C" {
    int resolve_unit_graph(
        const char *input_json,
        char **out,
        char **err_out
    );
    void free_string(char *s);
}

using namespace nix;

static void prim_resolveUnitGraph(EvalState &state, const PosIdx pos,
                                   Value **args, Value &v) {
    state.forceAttrs(*args[0], pos,
        "while evaluating the argument to builtins.resolveUnitGraph");

    // Serialize the entire input attrset to JSON and hand it to Rust
    NixStringContext context;
    auto inputJson = printValueAsJSON(state, true, *args[0], pos, context, false);
    auto inputStr = inputJson.dump();

    char *resultJson = nullptr;
    char *errorMsg = nullptr;

    int rc = resolve_unit_graph(inputStr.c_str(), &resultJson, &errorMsg);

    if (rc != 0) {
        std::string err = errorMsg ? errorMsg : "unknown error";
        if (errorMsg) free_string(errorMsg);
        state.error<EvalError>("resolveUnitGraph: %s", err).atPos(pos).debugThrow();
    }

    // Parse the result JSON into a Nix value
    std::string result(resultJson);
    free_string(resultJson);

    parseJSON(state, result, v);
}

static RegisterPrimOp rp({
    .name = "resolveUnitGraph",
    .args = {"attrs"},
    .arity = 1,
    .doc = R"(
      Resolve a Cargo workspace via unit-graph into a build plan attrset.
      
      Accepts an attrset with:
      - manifestPath: path to Cargo.toml
      - target: optional target triple string
      - includeDev: whether to include dev dependencies (default: false)
      - features, allFeatures, noDefaultFeatures: feature controls
      - bin, package, members: build filtering options
      
      Returns a NixBuildPlan attrset compatible with build-from-unit-graph.nix.
    )",
    .fun = prim_resolveUnitGraph,
});
