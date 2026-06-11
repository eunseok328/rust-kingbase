use marksman_escape::Escape;
use regex::Regex;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::iter;
use std::str;

use crate::snake_to_camel;

const PG_TYPE_DAT: &str = include_str!("pg_type.dat");
const PG_RANGE_DAT: &str = include_str!("pg_range.dat");
const KINGBASE_MYSQL_TYPE_DAT: &str = include_str!("kingbase_mysql_type.dat");

#[derive(Clone)]
struct Type {
    name: String,
    variant: String,
    ident: String,
    kind: String,
    typtype: Option<String>,
    element: u32,
    schema: String,
    doc: String,
}

pub fn build() {
    let types = parse_types();
    let mysql_overlay_types = parse_kingbase_mysql_overlay_types();
    let mysql_types = build_mysql_types(&types, &mysql_overlay_types);

    let mut pg_file = BufWriter::new(File::create("../postgres-types/src/pg_type_gen.rs").unwrap());
    make_mode_header(&mut pg_file, Mode::Pg);
    make_mode_enum(&mut pg_file, &types);
    make_pg_impl(&mut pg_file, &types);

    let mut mysql_file =
        BufWriter::new(File::create("../postgres-types/src/mysql_type_gen.rs").unwrap());
    make_mode_header(&mut mysql_file, Mode::Mysql);
    make_mode_enum(&mut mysql_file, &mysql_types);
    make_mysql_impl(&mut mysql_file, &mysql_types);

    let mut wrapper_file =
        BufWriter::new(File::create("../postgres-types/src/type_gen.rs").unwrap());
    make_wrapper_header(&mut wrapper_file);
    make_wrapper_enum(&mut wrapper_file);
    make_wrapper_impl(&mut wrapper_file);
    make_consts(&mut wrapper_file, &types, &mysql_types);
}

struct DatParser<'a> {
    it: iter::Peekable<str::CharIndices<'a>>,
    s: &'a str,
}

impl<'a> DatParser<'a> {
    fn new(s: &'a str) -> DatParser<'a> {
        DatParser {
            it: s.char_indices().peekable(),
            s,
        }
    }

    fn parse_array(&mut self) -> Vec<HashMap<String, String>> {
        self.eat('[');
        let mut vec = vec![];
        while !self.try_eat(']') {
            let object = self.parse_object();
            vec.push(object);
        }
        self.eof();

        vec
    }

    fn parse_object(&mut self) -> HashMap<String, String> {
        let mut object = HashMap::new();

        self.eat('{');
        loop {
            let key = self.parse_ident();
            self.eat('=');
            self.eat('>');
            let value = self.parse_string();
            object.insert(key, value);
            if !self.try_eat(',') {
                break;
            }
        }
        self.eat('}');
        self.eat(',');

        object
    }

    fn parse_ident(&mut self) -> String {
        self.skip_ws();

        let start = match self.it.peek() {
            Some((i, _)) => *i,
            None => return "".to_string(),
        };

        loop {
            match self.it.peek() {
                Some((_, 'a'..='z')) | Some((_, '_')) => {
                    self.it.next();
                }
                Some((i, _)) => return self.s[start..*i].to_string(),
                None => return self.s[start..].to_string(),
            }
        }
    }

    fn parse_string(&mut self) -> String {
        self.skip_ws();

        let mut s = String::new();

        self.eat('\'');
        loop {
            match self.it.next() {
                Some((_, '\'')) => return s,
                Some((_, '\\')) => {
                    let (_, ch) = self.it.next().expect("unexpected eof");
                    s.push(ch);
                }
                Some((_, ch)) => s.push(ch),
                None => panic!("unexpected eof"),
            }
        }
    }

    fn eat(&mut self, target: char) {
        self.skip_ws();

        match self.it.next() {
            Some((_, ch)) if ch == target => {}
            Some((_, ch)) => panic!("expected {} but got {}", target, ch),
            None => panic!("expected {} but got eof", target),
        }
    }

    fn try_eat(&mut self, target: char) -> bool {
        if self.peek(target) {
            self.eat(target);
            true
        } else {
            false
        }
    }

    fn peek(&mut self, target: char) -> bool {
        self.skip_ws();

        matches!(self.it.peek(), Some((_, ch)) if *ch == target)
    }

    fn eof(&mut self) {
        self.skip_ws();
        if let Some((_, ch)) = self.it.next() {
            panic!("expected eof but got {}", ch);
        }
    }

    fn skip_ws(&mut self) {
        loop {
            match self.it.peek() {
                Some(&(_, '#')) => self.skip_to('\n'),
                Some(&(_, '\n')) | Some(&(_, ' ')) | Some(&(_, '\t')) => {
                    self.it.next();
                }
                _ => break,
            }
        }
    }

    fn skip_to(&mut self, target: char) {
        for (_, ch) in &mut self.it {
            if ch == target {
                break;
            }
        }
    }
}

fn parse_types() -> BTreeMap<u32, Type> {
    let raw_types = DatParser::new(PG_TYPE_DAT).parse_array();
    let raw_ranges = DatParser::new(PG_RANGE_DAT).parse_array();

    let oids_by_name = raw_types
        .iter()
        .map(|m| (m["typname"].clone(), m["oid"].parse::<u32>().unwrap()))
        .collect::<HashMap<_, _>>();

    let range_elements = raw_ranges
        .iter()
        .map(|m| {
            (
                oids_by_name[&*m["rngtypid"]],
                oids_by_name[&*m["rngsubtype"]],
            )
        })
        .collect::<HashMap<_, _>>();
    let multi_range_elements = raw_ranges
        .iter()
        .map(|m| {
            (
                oids_by_name[&*m["rngmultitypid"]],
                oids_by_name[&*m["rngsubtype"]],
            )
        })
        .collect::<HashMap<_, _>>();

    let range_vector_re = Regex::new("(range|vector)$").unwrap();
    let array_re = Regex::new("^_(.*)").unwrap();

    let mut types = BTreeMap::new();

    for raw_type in raw_types {
        let oid = raw_type["oid"].parse::<u32>().unwrap();

        let name = raw_type["typname"].clone();

        let ident = range_vector_re.replace(&name, "_$1");
        let ident = array_re.replace(&ident, "${1}_array");
        let variant = snake_to_camel(&ident);
        let ident = ident.to_ascii_uppercase();

        let kind = raw_type["typcategory"].clone();

        // we need to be able to pull composite fields and enum variants at runtime
        if kind == "C" || kind == "E" {
            continue;
        }

        let typtype = raw_type.get("typtype").cloned();

        let element = match &*kind {
            "R" => match typtype
                .as_ref()
                .expect("range type must have typtype")
                .as_str()
            {
                "r" => range_elements[&oid],
                "m" => multi_range_elements[&oid],
                typtype => panic!("invalid range typtype {}", typtype),
            },
            "A" => oids_by_name[&raw_type["typelem"]],
            _ => 0,
        };

        let doc_name = array_re.replace(&name, "$1[]").to_ascii_uppercase();
        let mut doc = doc_name.clone();
        if let Some(descr) = raw_type.get("descr") {
            write!(doc, " - {descr}").unwrap();
        }
        let doc = Escape::new(doc.as_bytes().iter().cloned()).collect();
        let doc = String::from_utf8(doc).unwrap();

        if let Some(array_type_oid) = raw_type.get("array_type_oid") {
            let array_type_oid = array_type_oid.parse::<u32>().unwrap();

            let name = format!("_{name}");
            let variant = format!("{variant}Array");
            let doc = format!("{doc_name}&#91;&#93;");
            let ident = format!("{ident}_ARRAY");

            let type_ = Type {
                name,
                variant,
                ident,
                kind: "A".to_string(),
                typtype: None,
                element: oid,
                schema: "pg_catalog".to_string(),
                doc,
            };
            types.insert(array_type_oid, type_);
        }

        let type_ = Type {
            name,
            variant,
            ident,
            kind,
            typtype,
            element,
            schema: "pg_catalog".to_string(),
            doc,
        };
        types.insert(oid, type_);
    }

    types
}

fn parse_kingbase_mysql_overlay_types() -> BTreeMap<u32, Type> {
    let raw_types = DatParser::new(KINGBASE_MYSQL_TYPE_DAT).parse_array();

    let mut types = BTreeMap::new();
    for raw_type in raw_types {
        let oid = raw_type["oid"].parse::<u32>().unwrap();
        let name = raw_type["typname"].clone();
        let variant = raw_type
            .get("variant")
            .cloned()
            .unwrap_or_else(|| snake_to_camel(&name));
        let ident = raw_type
            .get("ident")
            .cloned()
            .unwrap_or_else(|| name.to_ascii_uppercase());
        let kind = raw_type["typcategory"].clone();
        let element = raw_type
            .get("typbasetype")
            .map(|base_oid| base_oid.parse::<u32>().unwrap())
            .unwrap_or(0);
        let schema = raw_type
            .get("schema")
            .cloned()
            .unwrap_or_else(|| "sys".to_string());

        let mut doc = format!("{schema}.{name}");
        if let Some(descr) = raw_type.get("descr") {
            write!(doc, " - {descr}").unwrap();
        }
        let doc = Escape::new(doc.as_bytes().iter().cloned()).collect();
        let doc = String::from_utf8(doc).unwrap();

        let type_ = Type {
            name,
            variant,
            ident,
            kind,
            typtype: None,
            element,
            schema,
            doc,
        };
        types.insert(oid, type_);
    }

    types
}

fn build_mysql_types(
    pg_types: &BTreeMap<u32, Type>,
    overlay_types: &BTreeMap<u32, Type>,
) -> BTreeMap<u32, Type> {
    let mut types = BTreeMap::new();

    // PG OIDs shared with the MySQL compatibility table.
    for oid in [
        // original
        16, 17, 19, 20, 21, 23, 25, 700, 701, 705, 1042, 1043, 1700, 3802,
        // PG-compatible arrays exposed by MysqlTypeInfoCache.
        199, 791, 1000, 1001, 1002, 1003, 1005, 1007, 1009, 1014, 1015, 1016, 1017, 1021, 1022,
        1028, 1115, 1182, 1183, 1185, 1231, 1270, 1561, 2201, 3807,
        // newly added
        18,   // char
        26,   // oid
        114,  // json   → PG_JSON
        600,  // point
        790,  // money
        1082, // date   → PG_DATE
        1083, // time   → PG_TIME
        1114, // timestamp → PG_TIMESTAMP
        1184, // timestamptz
        1266, // timetz
        1560, // bit    → PG_BIT
        1790, // refcursor
    ] {
        types.insert(oid, pg_types[&oid].clone());
    }

    for (oid, type_) in overlay_types {
        types.insert(*oid, type_.clone());
    }

    types
}

#[derive(Copy, Clone)]
enum Mode {
    Pg,
    Mysql,
}

fn make_mode_header(w: &mut BufWriter<File>, mode: Mode) {
    let extra_imports = match mode {
        Mode::Pg => "",
        Mode::Mysql => "",
    };

    write!(
        w,
        "// Autogenerated file - DO NOT EDIT
use crate::type_gen::Inner as TypeInner;
use crate::{{Kind, Oid, Type}};
{extra_imports}
"
    )
    .unwrap();
}

fn make_mode_enum(w: &mut BufWriter<File>, types: &BTreeMap<u32, Type>) {
    write!(
        w,
        "
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub enum Inner {{"
    )
    .unwrap();

    for type_ in types.values() {
        write!(
            w,
            "
    {},",
            type_.variant
        )
        .unwrap();
    }

    write!(
        w,
        r"
}}

"
    )
    .unwrap();
}

fn make_pg_impl(w: &mut BufWriter<File>, types: &BTreeMap<u32, Type>) {
    write!(
        w,
        "impl Inner {{
    pub fn from_oid(oid: Oid) -> Option<Inner> {{
        match oid {{
",
    )
    .unwrap();

    for (oid, type_) in types {
        writeln!(w, "            {} => Some(Inner::{}),", oid, type_.variant).unwrap();
    }

    writeln!(
        w,
        "            _ => None,
        }}
    }}

    pub fn oid(&self) -> Oid {{
        match *self {{",
    )
    .unwrap();

    for (oid, type_) in types {
        writeln!(w, "            Inner::{} => {},", type_.variant, oid).unwrap();
    }

    writeln!(
        w,
        "        }}
    }}

    pub fn kind(&self) -> &Kind {{
        match *self {{",
    )
    .unwrap();

    for type_ in types.values() {
        let kind = match &*type_.kind {
            "P" => "Pseudo".to_owned(),
            "A" => format!(
                "Array(Type(TypeInner::Pg(Inner::{})))",
                types[&type_.element].variant
            ),
            "M" => format!(
                "Domain(Type(TypeInner::Pg(Inner::{})))",
                types[&type_.element].variant
            ),
            "R" => match type_
                .typtype
                .as_ref()
                .expect("range type must have typtype")
                .as_str()
            {
                "r" => format!(
                    "Range(Type(TypeInner::Pg(Inner::{})))",
                    types[&type_.element].variant
                ),
                "m" => format!(
                    "Multirange(Type(TypeInner::Pg(Inner::{})))",
                    types[&type_.element].variant
                ),
                typtype => panic!("invalid range typtype {}", typtype),
            },
            _ => "Simple".to_owned(),
        };

        writeln!(
            w,
            "            Inner::{} => {{
                &Kind::{}
            }}",
            type_.variant, kind
        )
        .unwrap();
    }

    writeln!(
        w,
        r#"        }}
    }}

    pub fn name(&self) -> &str {{
        match *self {{"#,
    )
    .unwrap();

    for type_ in types.values() {
        writeln!(
            w,
            r#"            Inner::{} => "{}","#,
            type_.variant, type_.name
        )
        .unwrap();
    }

    writeln!(
        w,
        "        }}
    }}

    pub fn schema(&self) -> &str {{
        match *self {{",
    )
    .unwrap();

    for type_ in types.values() {
        writeln!(
            w,
            r#"            Inner::{} => "{}","#,
            type_.variant, type_.schema
        )
        .unwrap();
    }

    writeln!(
        w,
        "        }}
    }}
}}"
    )
    .unwrap();
}

fn make_mysql_impl(w: &mut BufWriter<File>, mysql_types: &BTreeMap<u32, Type>) {
    write!(
        w,
        "impl Inner {{
    pub fn from_oid(oid: Oid) -> Option<Inner> {{
    match oid {{
",
    )
    .unwrap();

    for (oid, type_) in mysql_types {
        writeln!(w, "        {} => Some(Inner::{}),", oid, type_.variant).unwrap();
    }

    writeln!(
        w,
        "        _ => None,
    }}
    }}

    pub fn oid(&self) -> Oid {{
        match *self {{",
    )
    .unwrap();

    for (oid, type_) in mysql_types {
        writeln!(w, "            Inner::{} => {},", type_.variant, oid).unwrap();
    }

    writeln!(
        w,
        "        }}
    }}

    pub fn kind(&self) -> &Kind {{
        match *self {{",
    )
    .unwrap();

    for type_ in mysql_types.values() {
        let kind = match &*type_.kind {
            "M" => {
                let base_type = mysql_mode_type_expr(type_.element, mysql_types);
                format!("Domain({base_type})")
            }
            "A" => {
                let member_type = mysql_mode_type_expr(type_.element, mysql_types);
                format!("Array({member_type})")
            }
            "R" => match type_
                .typtype
                .as_ref()
                .expect("range type must have typtype")
                .as_str()
            {
                "r" => {
                    let member_type = mysql_mode_type_expr(type_.element, mysql_types);
                    format!("Range({member_type})")
                }
                "m" => {
                    let member_type = mysql_mode_type_expr(type_.element, mysql_types);
                    format!("Multirange({member_type})")
                }
                typtype => panic!("invalid range typtype {}", typtype),
            },
            "P" => "Pseudo".to_owned(),
            _ => "Simple".to_owned(),
        };

        writeln!(
            w,
            "            Inner::{} => {{
                &Kind::{}
            }}",
            type_.variant, kind
        )
        .unwrap();
    }

    writeln!(
        w,
        r#"        }}
    }}

    pub fn name(&self) -> &str {{
        match *self {{"#,
    )
    .unwrap();

    for type_ in mysql_types.values() {
        writeln!(
            w,
            r#"            Inner::{} => "{}","#,
            type_.variant, type_.name
        )
        .unwrap();
    }

    writeln!(
        w,
        "        }}
    }}

    pub fn schema(&self) -> &str {{
        match *self {{",
    )
    .unwrap();

    for type_ in mysql_types.values() {
        writeln!(
            w,
            r#"            Inner::{} => "{}","#,
            type_.variant, type_.schema
        )
        .unwrap();
    }

    writeln!(
        w,
        "        }}
    }}
}}"
    )
    .unwrap();
}

fn mysql_mode_type_expr(oid: u32, mysql_types: &BTreeMap<u32, Type>) -> String {
    if let Some(type_) = mysql_types.get(&oid) {
        format!("Type(TypeInner::Mysql(Inner::{}))", type_.variant)
    } else {
        panic!("missing base oid {oid} for generated mysql type")
    }
}

fn make_wrapper_header(w: &mut BufWriter<File>) {
    write!(
        w,
        "// Autogenerated file - DO NOT EDIT
use std::sync::Arc;

use crate::{{Kind, Oid, Type}};

#[derive(PartialEq, Eq, Debug, Hash)]
pub struct Other {{
    pub name: String,
    pub oid: Oid,
    pub kind: Kind,
    pub schema: String,
}}
"
    )
    .unwrap();
}

fn make_wrapper_enum(w: &mut BufWriter<File>) {
    write!(
        w,
        "
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub enum Inner {{
    Pg(crate::pg_type_gen::Inner),
    Mysql(crate::mysql_type_gen::Inner),
    Other(Arc<Other>),
}}

"
    )
    .unwrap();
}

fn make_wrapper_impl(w: &mut BufWriter<File>) {
    write!(
        w,
        "impl Inner {{
    pub fn oid(&self) -> Oid {{
        match *self {{
            Inner::Pg(ref inner) => inner.oid(),
            Inner::Mysql(ref inner) => inner.oid(),
            Inner::Other(ref u) => u.oid,
        }}
    }}

    pub fn kind(&self) -> &Kind {{
        match *self {{
            Inner::Pg(ref inner) => inner.kind(),
            Inner::Mysql(ref inner) => inner.kind(),
            Inner::Other(ref u) => &u.kind,
        }}
    }}

    pub fn name(&self) -> &str {{
        match *self {{
            Inner::Pg(ref inner) => inner.name(),
            Inner::Mysql(ref inner) => inner.name(),
            Inner::Other(ref u) => &u.name,
        }}
    }}

    pub fn schema(&self) -> &str {{
        match *self {{
            Inner::Pg(ref inner) => inner.schema(),
            Inner::Mysql(ref inner) => inner.schema(),
            Inner::Other(ref u) => &u.schema,
        }}
    }}
}}
"
    )
    .unwrap();
}

fn make_consts(
    w: &mut BufWriter<File>,
    types: &BTreeMap<u32, Type>,
    mysql_types: &BTreeMap<u32, Type>,
) {
    write!(w, "impl Type {{").unwrap();
    for type_ in types.values() {
        writeln!(
            w,
            "
    /// {docs}
    pub const {ident}: Type = Type(Inner::Pg(crate::pg_type_gen::Inner::{variant}));",
            docs = type_.doc,
            ident = type_.ident,
            variant = type_.variant
        )
        .unwrap();
    }

    for mysql_type in mysql_types.values() {
        let ident = if mysql_type.schema != "pg_catalog"
            && types
                .values()
                .any(|pg_type| pg_type.ident == mysql_type.ident)
        {
            format!("SYS_{}", mysql_type.ident)
        } else {
            mysql_type.ident.clone()
        };
        write!(
            w,
            "

    /// {docs}
    pub const MYSQL_{ident}: Type = Type(Inner::Mysql(
        crate::mysql_type_gen::Inner::{variant},
    ));",
            docs = mysql_type.doc,
            ident = ident,
            variant = mysql_type.variant
        )
        .unwrap();
    }

    make_mysql_alias_consts(w);

    write!(w, "}}").unwrap();
}

fn make_mysql_alias_consts(w: &mut BufWriter<File>) {
    for (alias, target, docs) in [
        (
            "SMALLINT",
            "INT2",
            "MySQL SQL alias SMALLINT for the 2-byte integer type",
        ),
        (
            "SMALLINT_ARRAY",
            "INT2_ARRAY",
            "MySQL SQL alias SMALLINT[] for the 2-byte integer array type",
        ),
        (
            "INTEGER",
            "INT4",
            "MySQL SQL alias INTEGER for the 4-byte integer type",
        ),
        (
            "INTEGER_ARRAY",
            "INT4_ARRAY",
            "MySQL SQL alias INTEGER[] for the 4-byte integer array type",
        ),
        (
            "INT",
            "INT4",
            "MySQL SQL alias INT for the 4-byte integer type",
        ),
        (
            "INT_ARRAY",
            "INT4_ARRAY",
            "MySQL SQL alias INT[] for the 4-byte integer array type",
        ),
        (
            "BIGINT",
            "INT8",
            "MySQL SQL alias BIGINT for the 8-byte integer type",
        ),
        (
            "BIGINT_ARRAY",
            "INT8_ARRAY",
            "MySQL SQL alias BIGINT[] for the 8-byte integer array type",
        ),
        (
            "FLOAT",
            "FLOAT4",
            "MySQL SQL alias FLOAT for the single-precision floating point type",
        ),
        (
            "FLOAT_ARRAY",
            "FLOAT4_ARRAY",
            "MySQL SQL alias FLOAT[] for the single-precision floating point array type",
        ),
        (
            "DOUBLE",
            "FLOAT8",
            "MySQL SQL alias DOUBLE for the double-precision floating point type",
        ),
        (
            "DOUBLE_ARRAY",
            "FLOAT8_ARRAY",
            "MySQL SQL alias DOUBLE[] for the double-precision floating point array type",
        ),
        (
            "DECIMAL",
            "NUMERIC",
            "MySQL SQL alias DECIMAL for the arbitrary precision numeric type",
        ),
        (
            "DECIMAL_ARRAY",
            "NUMERIC_ARRAY",
            "MySQL SQL alias DECIMAL[] for the arbitrary precision numeric array type",
        ),
        (
            "DEC",
            "NUMERIC",
            "MySQL SQL alias DEC for the arbitrary precision numeric type",
        ),
        (
            "FIXED",
            "NUMERIC",
            "MySQL SQL alias FIXED for the arbitrary precision numeric type",
        ),
        (
            "BOOLEAN",
            "BOOL",
            "MySQL SQL alias BOOLEAN for the boolean type",
        ),
        (
            "BOOLEAN_ARRAY",
            "BOOL_ARRAY",
            "MySQL SQL alias BOOLEAN[] for the boolean array type",
        ),
    ] {
        writeln!(
            w,
            "
    /// {docs}
    pub const MYSQL_{alias}: Type = Type::MYSQL_{target};"
        )
        .unwrap();
    }
}
