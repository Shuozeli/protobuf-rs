use protoc_rs_schema::*;

pub fn find_msg<'a>(file: &'a FileDescriptorProto, name: &str) -> &'a DescriptorProto {
    file.message_type
        .iter()
        .find(|m| m.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("message '{}' not found", name))
}

pub fn find_field<'a>(msg: &'a DescriptorProto, name: &str) -> &'a FieldDescriptorProto {
    msg.field
        .iter()
        .find(|f| f.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("field '{}' not found in {:?}", name, msg.name))
}

pub fn find_enum<'a>(file: &'a FileDescriptorProto, name: &str) -> &'a EnumDescriptorProto {
    file.enum_type
        .iter()
        .find(|e| e.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("enum '{}' not found", name))
}

pub fn find_nested_msg<'a>(msg: &'a DescriptorProto, name: &str) -> &'a DescriptorProto {
    msg.nested_type
        .iter()
        .find(|m| m.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("nested message '{}' not found in {:?}", name, msg.name))
}

pub fn find_nested_enum<'a>(msg: &'a DescriptorProto, name: &str) -> &'a EnumDescriptorProto {
    msg.enum_type
        .iter()
        .find(|e| e.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("nested enum '{}' not found in {:?}", name, msg.name))
}

pub fn find_service<'a>(
    file: &'a FileDescriptorProto,
    name: &str,
) -> &'a ServiceDescriptorProto {
    file.service
        .iter()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("service '{}' not found", name))
}

pub fn find_method<'a>(
    svc: &'a ServiceDescriptorProto,
    name: &str,
) -> &'a MethodDescriptorProto {
    svc.method
        .iter()
        .find(|m| m.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("method '{}' not found in {:?}", name, svc.name))
}

pub fn find_enum_value<'a>(
    e: &'a EnumDescriptorProto,
    name: &str,
) -> &'a EnumValueDescriptorProto {
    e.value
        .iter()
        .find(|v| v.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("enum value '{}' not found in {:?}", name, e.name))
}
