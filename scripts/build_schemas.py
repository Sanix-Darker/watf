#!/usr/bin/env python3
"""Generate the stable machine-readable interchange schemas without dependencies."""
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]

def document(name, properties, required, defs=None):
    result={"$schema":"https://json-schema.org/draft/2020-12/schema", "$id":f"urn:watf:{name}:1", "title":f"watf {name} v1", "type":"object", "additionalProperties":False, "properties":properties, "required":required}
    if defs: result["$defs"]=defs
    return result

def write(name, schema):
    (ROOT/"schemas"/(name+".schema.json")).write_text(json.dumps(schema,indent=2,sort_keys=True)+"\n")

def main():
    text={"type":"string"}
    string_array={"type":"array","items":text}
    arity={"enum":["none","one","two","optional","many","unknown"]}
    source=document("source",{"kind":{"enum":["builtin","man","help","completion","local_doc","catalog"]},"reference":text,"version":text,"sha256":{"type":"string","pattern":"^[0-9a-fA-F]{64}$"},"modified_unix":{"type":"integer","minimum":0},"bytes":{"type":"integer","minimum":0}},["kind","reference"])
    write("capability",document("capability",{"id":text,"command":{"type":"array","minItems":1,"maxItems":12,"items":text},"kind":{"enum":["command","option","input_field","example"]},"name":text,"summary":text,"aliases":string_array,"arity":arity,"required":{"type":"boolean"},"value_type":text,"choices":string_array,"source":source},["id","command","kind","name","summary","source"]))
    literal={"type":"string","maxLength":4096,"pattern":"^[^\\u0000-\\u001f\\u007f]*$"}
    redirect={"type":"object","additionalProperties":False,"properties":{"mode":{"enum":["truncate","append"]},"path":dict(literal,minLength=1)},"required":["mode","path"]}
    step={"type":"object","additionalProperties":False,"properties":{"command":dict(literal,minLength=1),"args":{"type":"array","maxItems":64,"items":literal},"after":{"enum":["start","success","always","pipe"]},"stdout":{"anyOf":[{"type":"null"},redirect]}},"required":["command","args","after","stdout"]}
    plan=document("plan",{"status":{"enum":["ok","needs_input","unsupported"]},"steps":{"type":"array","maxItems":8,"items":step},"questions":{"type":"array","maxItems":8,"items":literal}},["status","steps","questions"])
    plan["allOf"]=[{"if":{"properties":{"status":{"const":"ok"}}},"then":{"properties":{"steps":{"minItems":1},"questions":{"maxItems":0}}},"else":{"properties":{"steps":{"maxItems":0}}}}]
    write("plan",plan)
    request=document("request",{"schema_version":{"const":1},"id":{"type":"string","maxLength":64},"op":{"enum":["search","route","run","exec","resolve_exec"]},"query":{"type":"string","maxLength":16384},"argv":{"type":"array","maxItems":76,"items":literal},"plan":{"anyOf":[{"type":"null"},plan]},"cwd":{"anyOf":[text,{"type":"null"}]},"timeout_ms":{"type":"integer","minimum":1,"maximum":600000},"max_output_bytes":{"type":"integer","minimum":256,"maximum":1048576},"raw_output_dir":{"anyOf":[text,{"type":"null"}]},"max_bytes":{"type":"integer","minimum":256,"maximum":1048576},"limit":{"type":"integer","minimum":1,"maximum":128},"catalog":{"type":"boolean"},"fields":{"type":"boolean"},"installed_only":{"type":"boolean"},"command":{"anyOf":[text,{"type":"null"}]}},["id"])
    request["allOf"]=[{"if":{"properties":{"op":{"const":"resolve_exec"}},"required":["op"]},"then":{"required":["query"],"properties":{"query":{"minLength":1}},"not":{"anyOf":[{"required":["argv"]},{"required":["plan"]}]}}}]
    write("request",request)
    evidence={"type":"object","additionalProperties":False,"properties":{"id":text,"command":text,"kind":{"enum":["command","option","input_field","example"]},"name":text,"aliases":string_array,"arity":arity,"summary":text,"source":{"type":"integer","minimum":0},"program_available":{"type":"boolean"},"clauses":{"type":"array","items":{"type":"integer","minimum":0}}},"required":["id","command","kind","name","arity","summary","source","program_available","clauses"]}
    provenance=dict(source)
    provenance.pop("$id",None)
    provenance["properties"]=dict(source["properties"],freshness={"enum":["snapshot","metadata_unchanged","changed","missing","unknown"]})
    provenance["required"]=source["required"]+["freshness"]
    packet=document("packet",{"id":text,"schema_version":{"const":1},"status":{"enum":["evidence","partial","no_evidence"]},"evidence":{"type":"array","items":evidence},"sources":{"type":"array","items":provenance},"clause_count":{"type":"integer","minimum":1,"maximum":8},"uncovered_clauses":{"type":"array","items":{"type":"integer","minimum":0}},"truncated":{"type":"boolean"},"estimated_tokens":{"type":"integer","minimum":0},"token_estimator":{"const":"utf8_bytes_div4_not_model_tokens"}},["schema_version","status","evidence","sources","clause_count","uncovered_clauses","truncated","estimated_tokens","token_estimator"])
    write("packet",packet)
    print("wrote 4 JSON schemas")
if __name__=="__main__":main()
