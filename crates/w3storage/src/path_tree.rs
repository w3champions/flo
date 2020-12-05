use serde::Serialize;
use std::path::{Component, Path, MAIN_SEPARATOR};

use crate::error::{Error, Result};

#[derive(Debug, Serialize)]
pub struct PathTree<'a> {
  pub root: Node<'a>,
}

impl<'a> PathTree<'a> {
  pub fn to_paths(&self) -> Vec<String> {
    self.root.to_paths("")
  }
}

impl<'a> PathTree<'a> {
  pub fn from_paths<T>(paths: &'a [T]) -> Result<Self>
  where
    T: AsRef<str>,
  {
    let mut paths_sorted: Vec<_> = paths.into_iter().map(|i| i.as_ref()).collect();
    paths_sorted.sort();
    paths_sorted.dedup();
    let mut root = Node {
      name: NodeName::Root,
      children: vec![],
    };
    for path_str in paths_sorted {
      let path = Path::new(path_str);
      let mut parent = &mut root;
      for component in path.components() {
        match component {
          Component::Normal(name) => {
            let name_str = name.to_str().expect("name str");
            let pos = parent.children.iter().position(|node| {
              if let NodeName::Name(value) = node.name {
                value == name_str
              } else {
                false
              }
            });
            if let Some(pos) = pos {
              parent = &mut parent.children[pos];
            } else {
              parent.children.push(Node::new(name_str));
              parent = parent.children.last_mut().unwrap();
            }
          }
          _ => return Err(Error::InvalidPath(path_str.to_string())),
        }
      }
    }

    Ok(PathTree { root })
  }
}

#[derive(Debug, Serialize)]
pub struct Node<'a> {
  pub name: NodeName<'a>,
  pub children: Vec<Node<'a>>,
}

impl<'a> Node<'a> {
  fn new(name: &'a str) -> Self {
    Node {
      name: NodeName::Name(name),
      children: Vec::new(),
    }
  }

  pub fn to_paths(&self, prefix: &str) -> Vec<String> {
    if self.children.is_empty() {
      vec![format!("{}{}{}", prefix, MAIN_SEPARATOR, self.name.as_ref())]
    } else {
      let name = self.name.as_ref();
      let prefix = if prefix.is_empty() {
        name.to_string()
      } else {
        format!("{}{}{}", prefix, MAIN_SEPARATOR, name)
      };
      self
        .children
        .iter()
        .flat_map(|child| child.to_paths(&prefix))
        .collect()
    }
  }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum NodeName<'a> {
  Root,
  Name(&'a str),
}

impl<'a> AsRef<str> for NodeName<'a> {
  fn as_ref(&self) -> &str {
    match *self {
      NodeName::Root => "",
      NodeName::Name(name) => name,
    }
  }
}

#[test]
fn test_path_tree() {
  use crate::W3Storage;
  use flo_platform::ClientPlatformInfo;
  let p = ClientPlatformInfo::from_env().unwrap();
  let s = W3Storage::new(&p).unwrap();
  let mut paths = s.list_storage_files(&format!("maps{}*", MAIN_SEPARATOR)).unwrap();
  paths.sort();
  paths.dedup();

  let tree = PathTree::from_paths(&paths).unwrap();
  let paths_ = tree.to_paths();
  assert_eq!(paths, paths_)
}
