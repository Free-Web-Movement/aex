DSL
    ::= '[' FieldRule* ']'

FieldRule
    ::= '{' Source ',' FieldName ',' Type ',' Required ',' IsArray (',' Constraints)? (',' Fields)? '}'

Source
    ::= 'source' ':' ('body' | 'query' | 'params')

FieldName
    ::= 'field' ':' STRING

Type
    ::= 'type' ':' ('string' | 'int' | 'float' | 'bool' | 'object')

Required
    ::= 'required' ':' ('true' | 'false')

IsArray
    ::= 'is_array' ':' ('true' | 'false')

Constraints
    ::= 'constraints' ':' '{' Length? (',' Value)? (',' Regex)? '}'

Length
    ::= 'length' ':' '{' ('min' ':' NUMBER)? (',' 'max' ':' NUMBER)? '}'

Value
    ::= 'value' ':' '{' ('min' ':' NUMBER)? (',' 'max' ':' NUMBER)? '}'

Regex
    ::= 'regex' ':' STRING

Fields
    ::= 'fields' ':' '[' FieldRule* ']'

STRING
    ::= '"' [^"]* '"'

NUMBER
    ::= DIGIT+

DIGIT
    ::= '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9'
