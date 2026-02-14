# DSL说明

DSL用于描述HTTP等少量动态输入数据。

它的特点是规定数据名称，值域都相关内容，对输入的数据进行有效的验证。

下面是aex框架所能支持的dsl描述方式：

```

(
    // 基本类型 + 范围 + 正则
    username:string[3,20] regex("^[a-zA-Z0-9_]+$"),  
    age:int[0,150]=30,                         // 默认值
    age:int=30,                         // 默认值
    score:float(0,100),                        // 范围闭区间 / 开区间混合
    active:bool=true,                           // 布尔类型 + 默认值

    // 可选字段
    nickname?:string[0,20],

    // 枚举
    role:string enum("admin","user","guest")=user,  

    // 联合类型
    id:int|float,                               // 可以是 int 或 float

    // 对象子规则（递归）
    profile:object(
        first_name:string[1,50],
        last_name:string[1,50],
        contact:object(
            email:string regex("^[^@\\s]+@[^@\\s]+\\.[^@\\s]+$"),
            phone?:string[0,20]
        )
    ),

    // 数组子规则
    tags:array<string[1,10]>,                  // 数组元素规则
    scores:array<int[0,100]>                   // 数组元素范围
)

```

```
(
    username:string[3,20] regex("^[a-zA-Z0-9_]+$"),  
    age:int[0,150]=30,      
    age:int=30,    
    score:float(0,100),                        
    active:bool=true,                    
    nickname?:string[0,20],
    role:string enum("admin","user","guest")=user,  
    id:int|float,                              
    profile:object(
        first_name:string[1,50],
        last_name:string[1,50],
        contact:object(
            email:string regex("^[^@\\s]+@[^@\\s]+\\.[^@\\s]+$"),
            phone?:string[0,20]
        )
    ),

    tags:array<string[1,10]>,            
    scores:array<int[0,100]>,
    distance:float[1.47e11,1.52e11]=1.496e11           
)
```
