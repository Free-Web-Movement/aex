#[macro_export]
macro_rules! frame {
    ($name:ident { $($field:vis $fname:ident : $fty:ty),* $(,)? }) => {
        #[derive(serde::Serialize, serde::Deserialize, bincode::Encode, bincode::Decode, Debug)]
        pub struct $name {
            pub version: u16,
            pub length: u32,
            pub data: Vec<u8>,
            $($field $fname : $fty),*
        }

        impl $crate::tcp::Frame for $name {
            fn version(&self) -> u16 { self.version }
            fn length(&self) -> u32 { self.length }
            fn payload(&self) -> &[u8] { &self.data }
        }
    };
}

#[macro_export]
macro_rules! command {
    ($name:ident { $($field:vis $fname:ident : $fty:ty),* $(,)? }) => {
        #[derive(serde::Serialize, serde::Deserialize, bincode::Encode, bincode::Decode, Debug)]
        pub struct $name {
            pub _id: u32, // 内部存储 ID
            pub version: u16,
            pub length: u32,
            pub data: Vec<u8>,
            $($field $fname : $fty),*
        }

        impl $crate::tcp::Command for $name {
            fn id(&self) -> u32 { self._id }
            fn version(&self) -> u16 { self.version }
            fn length(&self) -> u32 { self.length }
            fn validate(&self) -> bool { true }
        }
    };
}

#[macro_export]
macro_rules! codec {
    ($name:ident) => {
        impl $crate::tcp::Codec for $name {}
    };
}

#[macro_export]
macro_rules! router {
    ($cmd_ty:ty, { $($id:expr => $handler:expr),* $(,)? }) => {
        {
            let mut r = $crate::tcp::Router::<$cmd_ty>::new();
            $(
                r.on($id, $handler);
            )*
            r
        }
    };
}