use pest::Parser;
use pest_derive::Parser;

use std::fs;
use std::collections::HashMap;
use std::env;
use std::process;

use std::sync::Arc;

use neo4j::address::Address;
use neo4j::driver::auth::AuthToken;
use neo4j::driver::{ConnectionConfig, Driver, DriverConfig, EagerResult, RoutingControl};
use neo4j::retry::{ExponentialBackoff, RetryError};

#[derive(Parser)]
#[grammar = "ame_grammar.pest"]
struct AmeParser;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2{
        panic!("ERROR: Ame file name not provided!");
    }

    if args[1] == "--version"{
        println!("Ame v0.1.0");
        process::exit(0);
    }


    // AST abstractions
    let mut setup_config = SetupConfig::new();
    let mut lang_context:Vec<String> = vec![];
    let mut symbol_table:HashMap<String,Word> = HashMap::new();
    let mut eq_table:Vec<(&str,&str)> = Vec::new();
    let mut is_a_table:Vec<(&str,&str)> = Vec::new();
    let mut part_of_table:Vec<(&str,&str)> = Vec::new();

    let mut queries:Vec<String> = Vec::new();

    let path = args[1].clone();
    let langfile = fs::read_to_string(path.clone()).expect("Cannot read file");

    let pry = AmeParser::parse(Rule::file,&langfile).expect("Failed parsing").next().unwrap();

    for pair in pry.into_inner(){

        match pair.as_rule() {
            Rule::decl => (),
            Rule::setup_config => {

                for sc in pair.into_inner(){
                    match sc.as_rule() {
                        Rule::host => {
                            setup_config.host = sc.into_inner().next().unwrap().as_str();
                        },
                        Rule::port => {
                            setup_config.port = sc.into_inner().next().unwrap().as_str().parse().unwrap();
                        },
                        Rule::user => {
                            setup_config.user = sc.into_inner().next().unwrap().as_str();
                        },
                        Rule::password => {
                            setup_config.password = sc.into_inner().next().unwrap().as_str();
                        },
                        Rule::database => {
                            setup_config.database = sc.into_inner().next().unwrap().as_str();
                        },
                        _ => ()
                    }
                }

            },
            Rule::language => {
                for lg in pair.into_inner(){
                    lang_context.push(String::from(lg.as_str()));
                }
            },
            Rule::del_all => {
                queries.push(String::from("MATCH (n) DETACH DELETE (n)"));
            },
            Rule::statement => {
                for st in pair.into_inner(){
                    match st.as_rule() {
                        Rule::word_decl => {
                            let mut var = String::new();
                            let mut w = Word::new();
                            for wd in st.into_inner(){
                                match wd.as_rule() {
                                    Rule::lang_code => {
                                        if lang_context.contains(&wd.as_str().to_string()){
                                            w.lang = wd.as_str().to_string();
                                        }
                                        else{
                                            panic!("ERROR: Invalid language code {} not declared in the Language clause.",wd.as_str());
                                        }
                                    },
                                    Rule::variable => var = wd.as_str().to_string(),
                                    Rule::word_val => w.name = wd.as_str().to_string(),
                                    Rule::pos => w.pos = wd.as_str().to_string(),
                                    _ => ()
                                }
                            }
                            symbol_table.insert(var,w);
                        },
                        Rule::eq_decl => {
                            let mut var:Vec<&str> = vec![];
                            for eq in st.into_inner(){
                                if symbol_table.contains_key(eq.as_str()){
                                    var.push(eq.as_str());
                                }
                                else{
                                    panic!("ERROR: variable {} is used before being defined.",eq.as_str());
                                }
                            }
                            eq_table.push((var[0],var[1]));
                        },
                        Rule::is_a_decl => {
                            let mut var:Vec<&str> = vec![];
                            for is_a in st.into_inner(){
                                if symbol_table.contains_key(is_a.as_str()){
                                    var.push(is_a.as_str());
                                }
                                else{
                                    panic!("ERROR: variable {} is used before being defined.",is_a.as_str());
                                }
                            }
                            is_a_table.push((var[0],var[1]));
                        },
                        Rule::part_of_decl => {
                            let mut var:Vec<&str> = vec![];
                            for part_of in st.into_inner(){
                                if symbol_table.contains_key(part_of.as_str()){
                                    var.push(part_of.as_str());
                                }
                                else{
                                    panic!("ERROR: variable {} is used before being defined.",part_of.as_str());
                                }
                            }
                            part_of_table.push((var[0],var[1]));
                        },
                        _ => ()
                    }
                }
            },
            Rule::EOI|_ => ()
        }
    }

    // Generate Cypher queries from the AST abstractions
    queries.extend(lang_context.iter().map(|x| {qgen_lang_context(x)}).collect::<Vec<_>>());
    queries.extend(symbol_table.iter().map(|x1| {qgen_symbol_table(x1)}).collect::<Vec<_>>());
    queries.extend(eq_table.iter().map(|x| {qgen_eq_table(x,&symbol_table)}).collect::<Vec<_>>());
    queries.extend(is_a_table.iter().map(|x| {qgen_is_a_table(x,&symbol_table)}).collect::<Vec<_>>());
    queries.extend(part_of_table.iter().map(|x| {qgen_part_of_table(x,&symbol_table)}).collect::<Vec<_>>());

    // Connecting to neo4j
    let database = Arc::new(String::from(setup_config.database));
    let address = Address::from((setup_config.host, setup_config.port));
    let auth_token = AuthToken::new_basic_auth(setup_config.user, setup_config.password);
    let driver = Driver::new(
        ConnectionConfig::new(address),
        DriverConfig::new().with_auth(Arc::new(auth_token)),
    );

    // Executing queries against the database
    for q in queries{
        let result = driver
            .execute_query(q)
            .with_database(database.clone())
            .with_routing_control(RoutingControl::Write)
            .run_with_retry(ExponentialBackoff::default());
    }

    println!("File {} executed successfully against database {}",path, *database);

}

#[derive(Debug)]
struct SetupConfig<'i> {
    host:&'i str,
    port:u16,
    user:&'i str,
    password:&'i str,
    database:&'i str,
}

impl <'i> SetupConfig <'i>{
    fn new() -> SetupConfig<'i>{
        SetupConfig{
            host : "localhost",
            port : 7687,
            user : "neo4j",
            password : "password",
            database : "neo4j"
        }
    }
}

#[derive(Debug)]
struct Word {
    lang : String,
    name : String,
    pos : String
}

impl Word{
    fn new() -> Word{
        Word{
            lang: String::new(),
            name: String::new(),
            pos : String::new()
        }
    }
}

// Cypher query generators
fn qgen_lang_context(lc : &str) -> String {
    format!("MERGE (root:ROOT)
MERGE (lang:{})
MERGE (lang)-[:ANCHOR]->(root)",lc)
}

fn qgen_symbol_table(ste: (&String,&Word)) -> String{
    format!("MERGE (word:Word{{name:'{}',pos:'{}'}})
MERGE (lang:{})
MERGE (word)-[:LANG]->(lang)",ste.1.name,ste.1.pos,ste.1.lang)
}

fn qgen_eq_table(eqe: &(&str,&str), sym_table:&HashMap<String,Word>) -> String{
    let w1 = sym_table.get(eqe.0).unwrap();
    let w2 = sym_table.get(eqe.1).unwrap();
    format!("MERGE (:{})<-[:LANG]-(word1:Word{{name:'{}',pos:'{}'}})
MERGE (:{})<-[:LANG]-(word2:Word{{name:'{}',pos:'{}'}})
MERGE (word1)-[:EQ]->(word2)
MERGE (word1)<-[:EQ]-(word2)",w1.lang,w1.name,w1.pos,w2.lang,w2.name,w2.pos)
}

fn qgen_is_a_table(isae: &(&str,&str), sym_table:&HashMap<String,Word>) -> String{
    let w1 = sym_table.get(isae.0).unwrap();
    let w2 = sym_table.get(isae.1).unwrap();
    format!("MERGE (:{})<-[:LANG]-(sub:Word{{name:'{}',pos:'{}'}})
MERGE (:{})<-[:LANG]-(super:Word{{name:'{}',pos:'{}'}})
MERGE (sub)-[:IS_A]->(super)",w1.lang,w1.name,w1.pos,w2.lang,w2.name,w2.pos)
}

fn qgen_part_of_table(poe: &(&str,&str), sym_table:&HashMap<String,Word>) -> String{
    let w1 = sym_table.get(poe.0).unwrap();
    let w2 = sym_table.get(poe.1).unwrap();
    format!("MERGE (:{})<-[:LANG]-(part:Word{{name:'{}',pos:'{}'}})
MERGE (:{})<-[:LANG]-(whole:Word{{name:'{}',pos:'{}'}})
MERGE (part)-[:PART_OF]->(whole)",w1.lang,w1.name,w1.pos,w2.lang,w2.name,w2.pos)
}