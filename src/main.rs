use pest::Parser;
use pest_derive::Parser;

use std::fs;
use std::collections::HashMap;
use std::env;
use std::process;
use std::io;
use std::io::Read;
use std::process::exit;
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
        println!("Ame v0.2.0");
        process::exit(0);
    }

    let path = args[1].clone();
    let langfile = fs::read_to_string(path.clone()).expect("Cannot read file");
    parse_ame_file(&path, &langfile);

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
#[derive(Clone)]
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

fn qgen_associated_with_table(poe: &(&str,&str), sym_table:&HashMap<String,Word>) -> String{
    let w1 = sym_table.get(poe.0).unwrap();
    let w2 = sym_table.get(poe.1).unwrap();
    format!("MERGE (:{})<-[:LANG]-(word1:Word{{name:'{}',pos:'{}'}})
MERGE (:{})<-[:LANG]-(word2:Word{{name:'{}',pos:'{}'}})
MERGE (word1)-[:ASSOCIATED_WITH]->(word2)",w1.lang,w1.name,w1.pos,w2.lang,w2.name,w2.pos)
}

fn qgen_anon_words(w: &Word) -> String{
    format!("MERGE (word:Word{{name:'{}',pos:'{}'}})
MERGE (lang:{})
MERGE (word)-[:LANG]->(lang)",w.name,w.pos,w.lang)
}

fn qgen_anon_eq_table(wp: &(Word,Word)) -> String {

    let (w1,w2) = wp;

    format!("MERGE (worda:Word{{name:'{}',pos:'{}'}})
MERGE (langa:{})
MERGE (worda)-[:LANG]->(langa)
MERGE (wordb:Word{{name:'{}',pos:'{}'}})
MERGE (langb:{})
MERGE (wordb)-[:LANG]->(langb)
MERGE (:{})<-[:LANG]-(word1:Word{{name:'{}',pos:'{}'}})
MERGE (:{})<-[:LANG]-(word2:Word{{name:'{}',pos:'{}'}})
MERGE (word1)-[:EQ]->(word2)
MERGE (word1)<-[:EQ]-(word2)",w1.name,w1.pos,w1.lang,w2.name,w2.pos,w2.lang,w1.lang,w1.name,w1.pos,w2.lang,w2.name,w2.pos)
}

fn qgen_anon_is_a_table(wp: &(Word,Word)) -> String {

    let (w1,w2) = wp;

    format!("MERGE (worda:Word{{name:'{}',pos:'{}'}})
MERGE (langa:{})
MERGE (worda)-[:LANG]->(langa)
MERGE (wordb:Word{{name:'{}',pos:'{}'}})
MERGE (langb:{})
MERGE (wordb)-[:LANG]->(langb)
MERGE (:{})<-[:LANG]-(sub:Word{{name:'{}',pos:'{}'}})
MERGE (:{})<-[:LANG]-(super:Word{{name:'{}',pos:'{}'}})
MERGE (sub)-[:IS_A]->(super)",w1.name,w1.pos,w1.lang,w2.name,w2.pos,w2.lang,w1.lang,w1.name,w1.pos,w2.lang,w2.name,w2.pos)
}

fn qgen_anon_part_of_table(wp: &(Word,Word)) -> String {

    let (w1,w2) = wp;

    format!("MERGE (worda:Word{{name:'{}',pos:'{}'}})
MERGE (langa:{})
MERGE (worda)-[:LANG]->(langa)
MERGE (wordb:Word{{name:'{}',pos:'{}'}})
MERGE (langb:{})
MERGE (wordb)-[:LANG]->(langb)
MERGE (:{})<-[:LANG]-(part:Word{{name:'{}',pos:'{}'}})
MERGE (:{})<-[:LANG]-(whole:Word{{name:'{}',pos:'{}'}})
MERGE (part)-[:PART_OF]->(whole)",w1.name,w1.pos,w1.lang,w2.name,w2.pos,w2.lang,w1.lang,w1.name,w1.pos,w2.lang,w2.name,w2.pos)
}

fn qgen_anon_associated_with_table(wp: &(Word,Word)) -> String {

    let (w1,w2) = wp;

    format!("MERGE (worda:Word{{name:'{}',pos:'{}'}})
MERGE (langa:{})
MERGE (worda)-[:LANG]->(langa)
MERGE (wordb:Word{{name:'{}',pos:'{}'}})
MERGE (langb:{})
MERGE (wordb)-[:LANG]->(langb)
MERGE (:{})<-[:LANG]-(word1:Word{{name:'{}',pos:'{}'}})
MERGE (:{})<-[:LANG]-(word2:Word{{name:'{}',pos:'{}'}})
MERGE (word1)-[:ASSOCIATED_WITH]->(word2)",w1.name,w1.pos,w1.lang,w2.name,w2.pos,w2.lang,w1.lang,w1.name,w1.pos,w2.lang,w2.name,w2.pos)
}

fn query_executor(path:&str,queries:&Vec<String>,setup_config: SetupConfig){
    // Connecting to neo4j
    let database = Arc::new(String::from(setup_config.database));
    let address = Address::from((setup_config.host, setup_config.port));
    let auth_token = AuthToken::new_basic_auth(setup_config.user, setup_config.password);
    let driver = Driver::new(
        ConnectionConfig::new(address),
        DriverConfig::new().with_auth(Arc::new(auth_token)),
    );

    // Executing queries against the database
    let lenq = queries.len();
    let mut cur = 0;
    for q in queries{
        let result = driver
            .execute_query(q.clone())
            .with_database(database.clone())
            .with_routing_control(RoutingControl::Write)
            .run_with_retry(ExponentialBackoff::default());

        result.expect(&format!("Failed executing query:\n {}",q));
        cur += 1;
        print!("\rExecuted query: ({}/{})",cur,lenq);
    }

    println!("\nCompleted! File {} executed successfully against database {}",path, *database);
}

fn parse_ame_file(path: &str, langfile:&str){
    // AST abstractions
    let mut setup_config = SetupConfig::new();
    let mut lang_context:Vec<String> = vec![];
    let mut symbol_table:HashMap<String,Word> = HashMap::new();
    let mut eq_table:Vec<(&str,&str)> = Vec::new();
    let mut is_a_table:Vec<(&str,&str)> = Vec::new();
    let mut part_of_table:Vec<(&str,&str)> = Vec::new();
    let mut associated_with_table:Vec<(&str,&str)> = Vec::new();

    let mut anon_words: Vec<Word> = vec![];
    let mut anon_eq_table:Vec<(Word,Word)> = Vec::new();
    let mut anon_is_a_table:Vec<(Word,Word)> = Vec::new();
    let mut anon_part_of_table:Vec<(Word,Word)> = Vec::new();
    let mut anon_associated_with_table:Vec<(Word,Word)> = Vec::new();

    let mut queries:Vec<String> = Vec::new();

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
                        Rule::associated_with_decl => {
                            let mut var:Vec<&str> = vec![];
                            for part_of in st.into_inner(){
                                if symbol_table.contains_key(part_of.as_str()){
                                    var.push(part_of.as_str());
                                }
                                else{
                                    panic!("ERROR: variable {} is used before being defined.",part_of.as_str());
                                }
                            }
                            associated_with_table.push((var[0],var[1]));
                        },
                        Rule::short_word_decl => {
                            let mut w = Word::new();
                            for swd in st.into_inner(){
                                match swd.as_rule() {
                                    Rule::short_word => {
                                        for sw in swd.into_inner(){
                                            match sw.as_rule() {
                                                Rule::lang_code => {
                                                    if lang_context.contains(&sw.as_str().to_string()){
                                                        w.lang = sw.as_str().to_string();
                                                    }
                                                    else{
                                                        panic!("ERROR: Invalid language code {} not declared in the Language clause.",sw.as_str());
                                                    }
                                                },
                                                Rule::word_val => w.name = sw.as_str().to_string(),
                                                Rule::pos => w.pos = sw.as_str().to_string(),
                                                _ => ()
                                            }
                                        }
                                    },
                                    _ => ()
                                }
                            }
                            anon_words.push(w);
                        },
                        Rule::short_eq_decl => {
                            let mut words:Vec<Word> = vec![];
                            for seq in st.into_inner(){
                                match seq.as_rule() {
                                    Rule::short_word => {
                                        let mut w = Word::new();
                                        for sw in seq.into_inner(){
                                            match sw.as_rule() {
                                                Rule::lang_code => {
                                                    if lang_context.contains(&sw.as_str().to_string()){
                                                        w.lang = sw.as_str().to_string();
                                                    }
                                                    else{
                                                        panic!("ERROR: Invalid language code {} not declared in the Language clause.",sw.as_str());
                                                    }
                                                },
                                                Rule::word_val => w.name = sw.as_str().to_string(),
                                                Rule::pos => w.pos = sw.as_str().to_string(),
                                                _ => ()
                                            }
                                        }
                                        words.push(w);
                                    },
                                    _ => ()
                                }
                            }
                            anon_eq_table.push((words[0].clone(),words[1].clone()));
                        },
                        Rule::short_is_a_decl => {
                            let mut words:Vec<Word> = vec![];
                            for seq in st.into_inner(){
                                match seq.as_rule() {
                                    Rule::short_word => {
                                        let mut w = Word::new();
                                        for sw in seq.into_inner(){
                                            match sw.as_rule() {
                                                Rule::lang_code => {
                                                    if lang_context.contains(&sw.as_str().to_string()){
                                                        w.lang = sw.as_str().to_string();
                                                    }
                                                    else{
                                                        panic!("ERROR: Invalid language code {} not declared in the Language clause.",sw.as_str());
                                                    }
                                                },
                                                Rule::word_val => w.name = sw.as_str().to_string(),
                                                Rule::pos => w.pos = sw.as_str().to_string(),
                                                _ => ()
                                            }
                                        }
                                        words.push(w);
                                    },
                                    _ => ()
                                }
                            }
                            anon_is_a_table.push((words[0].clone(),words[1].clone()));
                        },
                        Rule::short_part_of_decl => {
                            let mut words:Vec<Word> = vec![];
                            for seq in st.into_inner(){
                                match seq.as_rule() {
                                    Rule::short_word => {
                                        let mut w = Word::new();
                                        for sw in seq.into_inner(){
                                            match sw.as_rule() {
                                                Rule::lang_code => {
                                                    if lang_context.contains(&sw.as_str().to_string()){
                                                        w.lang = sw.as_str().to_string();
                                                    }
                                                    else{
                                                        panic!("ERROR: Invalid language code {} not declared in the Language clause.",sw.as_str());
                                                    }
                                                },
                                                Rule::word_val => w.name = sw.as_str().to_string(),
                                                Rule::pos => w.pos = sw.as_str().to_string(),
                                                _ => ()
                                            }
                                        }
                                        words.push(w);
                                    },
                                    _ => ()
                                }
                            }
                            anon_part_of_table.push((words[0].clone(),words[1].clone()));
                        },
                        Rule::short_associated_with_decl => {
                            let mut words:Vec<Word> = vec![];
                            for seq in st.into_inner(){
                                match seq.as_rule() {
                                    Rule::short_word => {
                                        let mut w = Word::new();
                                        for sw in seq.into_inner(){
                                            match sw.as_rule() {
                                                Rule::lang_code => {
                                                    if lang_context.contains(&sw.as_str().to_string()){
                                                        w.lang = sw.as_str().to_string();
                                                    }
                                                    else{
                                                        panic!("ERROR: Invalid language code {} not declared in the Language clause.",sw.as_str());
                                                    }
                                                },
                                                Rule::word_val => w.name = sw.as_str().to_string(),
                                                Rule::pos => w.pos = sw.as_str().to_string(),
                                                _ => ()
                                            }
                                        }
                                        words.push(w);
                                    },
                                    _ => ()
                                }
                            }
                            anon_associated_with_table.push((words[0].clone(),words[1].clone()));
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
    queries.extend(associated_with_table.iter().map(|x| {qgen_associated_with_table(x,&symbol_table)}).collect::<Vec<_>>());

    queries.extend(anon_words.iter().map(|x| {qgen_anon_words(x)}).collect::<Vec<_>>());
    queries.extend(anon_eq_table.iter().map(|x| {qgen_anon_eq_table(x)}).collect::<Vec<_>>());
    queries.extend(anon_is_a_table.iter().map(|x| {qgen_anon_is_a_table(x)}).collect::<Vec<_>>());
    queries.extend(anon_part_of_table.iter().map(|x| {qgen_anon_part_of_table(x)}).collect::<Vec<_>>());
    queries.extend(anon_associated_with_table.iter().map(|x| {qgen_anon_associated_with_table(x)}).collect::<Vec<_>>());


    query_executor(&path,&queries,setup_config);
}