extern crate serde_yaml;
extern crate serde_json;
extern crate serde;
extern crate linked_hash_map;

#[macro_use] extern crate lazy_static;
extern crate regex;

use regex::Regex;

use std::env;
use std::fs::File;

use serde_yaml::{Value, to_value};

fn to_pg_array(seq: &Value) -> String {
  let val = seq.as_sequence().unwrap();
  let val_str: String = val.iter()
                           .map(|s| "'".to_string() +  s.as_str().unwrap() + "', ")
                           .collect();

  if val_str.len() == 0 {
    return format!("array[]");
  } else {
    let (val_str, _) = val_str.split_at(val_str.len()-2);
    return format!("array[{}]", val_str);
  }
}

macro_rules! get_string_or_unnest {
    ($value:expr, $name:expr) => (match $value.get(&to_value($name)) {
        Some(&Value::Sequence(ref value)) => format!("unnest({})", to_pg_array(&to_value(&value))),
        Some(&Value::String(ref value)) => format!("'{}'", value.clone()),
        _ => panic!(format!("No '{}' found in permission entry", $name))
    })
}
macro_rules! get_string_or {
    ($value:expr, $name:expr, $or:expr) => (match $value.get(&to_value($name)) {
        Some(&Value::String(ref value)) => format!("'{}'", value.clone()),
        _ => $or.to_string()
    })
}

fn handle_fields(fields: & Value) {
  // Unpack the yaml and create insert statements
  // 

  let mut sql_query = String::new();
  sql_query.push_str("\n--#####################--\n-- Creating all fields --\n--#####################--\n\n");
  sql_query.push_str("DELETE FROM fields; ALTER SEQUENCE fields_id_seq RESTART WITH 1;");
  sql_query.push_str("\nINSERT INTO fields (ref_table, name, data, modified_by)\nVALUES\n--");
  for (ref key, ref value) in fields.as_mapping().unwrap() {
    // Take properties from value
    let ref_table = key.as_str().unwrap();
    let mut mut_data = value.to_owned().clone();
    let mut mapping = mut_data.as_mapping_mut().unwrap();

    // .as_mapping_mut().unwrap();
    let properties = mapping.remove(&to_value("properties")).unwrap();

    sql_query.push_str(format!(",\n\t('{}', NULL, '{}', -1)", ref_table, serde_json::to_string(mapping).unwrap()).as_str());

    properties.as_mapping().unwrap()
      .iter()
      .fold(&mut sql_query, |mut acc, (ref name, ref definition)| {
        acc.push_str(format!(",\n\t('{}', '{}', '{}', -1)", ref_table, name.as_str().unwrap(), serde_json::to_string(definition).unwrap()).as_str());
        acc
      });
  }
  sql_query.push_str(";");
  println!("{}", sql_query);
}
fn handle_people(people: & Value) {
  // Unpack the yaml and create insert statements

  let mut sql_query = String::new();

  sql_query.push_str("\n--#####################--\n-- Creating all people --\n--#####################--\n\n");
  sql_query.push_str("DELETE FROM people; ALTER SEQUENCE people_id_seq RESTART WITH 1;");
  sql_query.push_str("\nINSERT INTO people (email, phone, modified_by, data, password_hash)\nVALUES\n--");
  for ref value in people.as_sequence().unwrap() {
    // Take properties from value
    let mut mutvalue = value.to_owned().clone();
    let mut person = mutvalue.as_mapping_mut().unwrap();

    let email = person.remove(&to_value("email")).unwrap();
    let phone = person.remove(&to_value("phone")).unwrap_or(to_value(""));  
    // let password = person.remove(&to_value("password")).unwrap_or(to_value(""));  

    sql_query.push_str(format!(
      // ",\n\t('{}', '{}', -1, '{}', crypt('{}',gen_salt('bf',4)))",
      ",\n\t('{}', '{}', -1, '{}', NULL)",
      email.as_str().unwrap(),
      phone.as_str().unwrap(),
      serde_json::to_string(person).unwrap().as_str()
      // password.as_str().unwrap()
    ).as_str());
  }
  sql_query.push_str(";");
  println!("{}", sql_query);
}
fn handle_roles(roles: & Value) {
  // Takes roles, and inserts those
  // Creates the people_roles out of members for each role.

  // INSERT INTO roles (name, modified_by) VALUES

  let mut sql_query = String::new();
  let mut member_alias = String::new();
  member_alias.push_str("--Catch comma");


  sql_query.push_str("\n--####################--\n-- Creating all roles --\n--####################--\n\n");
  sql_query.push_str("DELETE FROM roles; ALTER SEQUENCE roles_id_seq RESTART WITH 1;");
  sql_query.push_str("\nINSERT INTO roles (name, data, modified_by)\nVALUES\n--");
  for ref value in roles.as_sequence().unwrap() {
    // Take properties from value
    let mut mutvalue = value.to_owned().clone();
    let mut role = mutvalue.as_mapping_mut().unwrap();

    let name = role.remove(&to_value("name")).unwrap();
    let ref members = role.remove(&to_value("members")).unwrap();  

    sql_query.push_str(format!(
      ",\n\t('{}', '{}', -1)",
      name.as_str().unwrap(),
      serde_json::to_string(role).unwrap().as_str()
      ).as_str());

    // Assign member alias
    if members.as_sequence().unwrap().len() != 0 {
      member_alias.push_str(format!(",\n\t\t('{}', {})", name.as_str().unwrap(), to_pg_array(members)).as_str());
    }
  }
  sql_query.push_str(";");

  sql_query.push_str("\n--###########################--\n-- Creating all people_roles --\n--###########################--\n\n");
  sql_query.push_str("DELETE FROM people_roles;");

  sql_query.push_str("INSERT INTO people_roles (people_id, roles_id, modified_by)\n");
  sql_query.push_str("SELECT people.id, roles.id, -1 FROM\n");
  sql_query.push_str("\t(VALUES\n");
  sql_query.push_str(member_alias.as_str());
  sql_query.push_str("\t) alias (role_name, people_emails)\n");
  sql_query.push_str("JOIN roles ON roles.valid_till IS NULL AND roles.name = alias.role_name\n");
  sql_query.push_str("JOIN people ON people.valid_till IS NULL AND people.email IN (SELECT unnest(alias.people_emails));");

  println!("{}", sql_query);
}
fn handle_permissions(permissions: & Value) {
  // Generate a couple of queries based on the items in permissions
  let mut sql_query = String::new();


  lazy_static! {
    static ref SQL_RE: Regex = Regex::new(r"(?s)'\s*SQL\((.*)\)\s*'").unwrap();
    static ref WHITESPACE_RE: Regex = Regex::new(r"\s+").unwrap();
  }


  sql_query.push_str(format!("\n--##########################--\n-- Creating all permissions --\n--##########################--\n\n").as_str());
  sql_query.push_str("DELETE FROM permissions; ALTER SEQUENCE permissions_id_seq RESTART WITH 1;");

  for permission in permissions.as_sequence().unwrap() {
    // println!("{}", serde_json::to_string_pretty(permission).unwrap().as_str());

    let permission = permission.as_mapping().unwrap();

    let p_table = get_string_or_unnest!(permission, "table");
    let p_type = format!("{}::permissions_type", get_string_or_unnest!(permission, "type"));
    let p_description = get_string_or!(permission, "description", "'Nothing'");
    let p_key = get_string_or!(permission, "key", "NULL");
    let p_value = get_string_or!(permission, "value", "NULL::int");

    sql_query.push_str(format!("\n\n-- {}\n", p_description).as_str());

    sql_query.push_str(format!("INSERT INTO permissions (type, ref_table, ref_key, ref_value, modified_by)\n").as_str());
    sql_query.push_str(format!("SELECT types.type, tables.ref_table, keys.ref_key, values.id, -1 as modified_by FROM\n").as_str());
    sql_query.push_str(format!("(SELECT {}::permissions_type AS type) types\n", p_type).as_str());
    sql_query.push_str(format!("CROSS JOIN (SELECT {} AS ref_table) tables\n", p_table).as_str());
    sql_query.push_str(format!("CROSS JOIN (SELECT {} AS ref_key) keys\n", p_key).as_str());


    let p_value = match SQL_RE.captures(p_value.clone().as_str()) {
      Some(value) => WHITESPACE_RE.replace_all(value.get(1).unwrap().as_str(), " ").into_owned(),
      _ => format!("CROSS JOIN (SELECT {} AS id) values", p_value)
    };
    sql_query.push_str(format!("{};\n", p_value).as_str());
  }

  println!("{}", sql_query);
}

fn handle_roles_permissions(permission_maps: & Value) {
  // Generate a couple of queries based on the items in permissions
  let mut sql_query = String::new();

  sql_query += format!("\n--##########################--\n-- Creating all permission mappings --\n--##########################--\n\n").as_str();
  sql_query += "DELETE FROM roles_permissions;";

  let mut field_perms = String::new();
  let mut global_create_perms = String::new();
  let mut other_create_perms = String::new();

  // For each role to map to:
  for (role, permissions) in permission_maps.as_mapping().unwrap().iter() {
    let role = role.as_str().unwrap();

    for (table, permissions) in permissions.as_mapping().unwrap().iter() {
      let table = table.as_str().unwrap();

      for (p_type, permission_set) in permissions.as_mapping().unwrap().iter() {
        let p_type = p_type.as_str().unwrap();
        match (p_type, permission_set)  {
          ("create", &Value::Mapping(ref create)) => {
            if create.is_empty() {
              // Filter all global create mapping
              global_create_perms += format!(
                "\t('{}', '{}', '{}'),\n",
                p_type,
                role,
                table
              ).as_str();
            } else {
              // It's a selective create mapping
              let mut create_iter = create.iter();
              while let Some((&Value::String(ref value_ref), roles)) = create_iter.next() {
                other_create_perms += format!(
                  "\t('{}', '{}_id', {}),\n",
                  role,
                  value_ref,
                  to_pg_array(roles)
                ).as_str();
              }
            }
          },
          ("edit", &Value::Sequence(_))
          | ("view", &Value::Sequence(_)) => {
            // Filter all fields permissions, Vec<String>
            field_perms += format!("\t('{}', '{}', '{}', {}),\n", p_type, role, table, to_pg_array(permission_set)).as_str();
          },
          (str, value) => println!("\t\t -- CUSTOM PERMISSION| Not handled: {:?} | {:?}", str, value),
        }
      }
    }
  }

  if field_perms.len() > 0 {
    // Strip the comma
    let (field_perms, _) = field_perms.split_at(field_perms.len()-2);

    sql_query += "\n-- Allowing the field permissions.\n";
    sql_query += "INSERT INTO roles_permissions (roles_id, permissions_id, modified_by)\n";
    sql_query += format!("
      SELECT DISTINCT roles.id, permissions.id, -1 FROM
      (VALUES
{}
      ) alias (p_type, p_role, f_table, fields_names)
      JOIN roles ON roles.valid_till IS NULL AND roles.name = alias.p_role
      JOIN fields ON fields.valid_till IS NULL AND fields.name IN (SELECT unnest(alias.fields_names))
      JOIN permissions ON
          permissions.valid_till IS NULL
          AND permissions.type::TEXT = alias.p_type
          AND permissions.ref_table = alias.f_table
          AND permissions.ref_key = 'fields'
          AND permissions.ref_value = fields.id
    ", field_perms).as_str();
  }

  if global_create_perms.len() > 0 {
    if field_perms.len() > 0 {
      sql_query += "\nUNION\n";
    }
    // Strip the comma
    let (global_create_perms, _) = global_create_perms.split_at(global_create_perms.len()-2);

    sql_query += "\n-- Allowing the global create permissions.\n";
    sql_query += format!("
      SELECT DISTINCT roles.id, permissions.id, -1 FROM
      (VALUES
{}
      ) alias (p_type, p_role, f_table)
      JOIN roles ON roles.valid_till IS NULL AND roles.name = alias.p_role
      JOIN permissions ON
          permissions.valid_till IS NULL
          AND permissions.type::TEXT = alias.p_type
          AND permissions.ref_table = alias.f_table
          AND permissions.ref_key IS NULL
          AND permissions.ref_value IS NULL
    ", global_create_perms).as_str();
  }

  if other_create_perms.len() > 0 {
    if (field_perms.len() > 0) || (global_create_perms.len() > 0) {
      sql_query += "\nUNION\n";
    }
    // Strip the comma
    let (other_create_perms, _) = other_create_perms.split_at(other_create_perms.len()-2);

    sql_query += "\n-- Allowing the specific create permissions.\n";
    sql_query += format!("
      SELECT DISTINCT roles.id, permissions.id, -1 
      FROM (VALUES 
{}
      ) alias (role, ref_key, subject_roles)
      JOIN roles
        ON roles.valid_till IS NULL
        AND roles.name = alias.role
      JOIN permissions
        ON permissions.valid_till IS NULL
        AND permissions.ref_table = 'people_roles'
        AND permissions.type::TEXT = 'create'
        AND permissions.ref_key = alias.ref_key
        AND permissions.ref_value IN (
          SELECT subject_roles.id FROM roles subject_roles
            WHERE subject_roles.valid_till IS NULL
            AND subject_roles.name IN (SELECT unnest(alias.subject_roles))
        )
    ", other_create_perms).as_str();
  }

  // Filter all people_roles mapping


  // println!("{}", serde_json::to_string_pretty(permission_maps).unwrap());
  // println!("{}", serde_yaml::to_string(permission_maps).unwrap());
  println!("{};", sql_query);

}


fn main() {
  // Read the file into a yaml value
  let filename: String = match env::args().nth(1) {
    Some(string) => string,
    _ => panic!("No argument supplied")
  };
  let file = File::open(filename).unwrap();
  let yaml_content: serde_yaml::Value = serde_yaml::from_reader(file).unwrap();

  // Loop over the initial sequences
  match env::args().nth(2) {
    Some(filter) => {
      for data in yaml_content.as_sequence().unwrap() {
        for (ref key, ref value ) in data.as_mapping().unwrap().iter() {
          if key.as_str().unwrap() == filter {
            match key.as_str() {
              Some("fields") => handle_fields(value),
              Some("people") => handle_people(value),
              Some("roles") => handle_roles(value),
              Some("permissions") => handle_permissions(value),
              Some("roles_permissions") => handle_roles_permissions(value),
              Some(_) | None => panic!("No key found")
            }
          }
        }
      }
    },
    _ => {
      for data in yaml_content.as_sequence().unwrap() {
        for (ref key, ref value ) in data.as_mapping().unwrap().iter() {

          match key.as_str() {
            Some("fields") => handle_fields(value),
            Some("people") => handle_people(value),
            Some("roles") => handle_roles(value),
            Some("permissions") => handle_permissions(value),
            Some("roles_permissions") => handle_roles_permissions(value),
            Some(_) | None => panic!("No key found")
          }
        }
      }
    }
  }
  // Map the keys on handler functions

  // println!("{:?}", yaml_content);
}