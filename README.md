<img src="./ame_dark_icon.png" height=100>

Ame is a parser for a custom language used to define a wordnet of sorts in Neo4j. It connects to a Neo4j database and takes simple natural language like commands, translates them into cypher queries and executes them against the Neo4j database.

The [pest file](./src/ame_grammar.pest) contains the Parsing Expression Grammar (PEG) for this language.

The [rust file](./src/main.rs) can be compiled into an executable that take a .ame file as input and translates and execute the Ame queries against a Neo4j database. The compiled executable is available [here](./target/release/ame.exe).