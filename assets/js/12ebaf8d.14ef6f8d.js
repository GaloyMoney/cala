"use strict";(self.webpackChunkwebsite=self.webpackChunkwebsite||[]).push([[1438],{7512:(e,n,t)=>{t.r(n),t.d(n,{assets:()=>o,contentTitle:()=>r,default:()=>h,frontMatter:()=>i,metadata:()=>c,toc:()=>d});var a=t(4848),s=t(8453);const i={id:"transaction-post",title:"Post a Transaction",slug:"/docs/transaction-post"},r=void 0,c={id:"demo/transaction-post",title:"Post a Transaction",description:"This functionality allows to execute financial transactions based on predefined parameters and templates. The specific transaction being posted here is based on a deposit template, which facilitates adding funds to a user's account.",source:"@site/docs/demo/transaction-post.mdx",sourceDirName:"demo",slug:"/docs/transaction-post",permalink:"/docs/transaction-post",draft:!1,unlisted:!1,tags:[],version:"current",frontMatter:{id:"transaction-post",title:"Post a Transaction",slug:"/docs/transaction-post"},sidebar:"demoSidebar",previous:{title:"Create Transaction Templates",permalink:"/docs/tx-template-create"},next:{title:"Check Account Balance",permalink:"/docs/check-account-balance"}},o={},d=[{value:"Process",id:"process",level:2},{value:"Variables",id:"variables",level:3},{value:"GraphQL Request Body",id:"graphql-request-body",level:3},{value:"Response",id:"response",level:2},{value:"Significance",id:"significance",level:2}];function l(e){const n={code:"code",h2:"h2",h3:"h3",li:"li",p:"p",pre:"pre",strong:"strong",ul:"ul",...(0,s.R)(),...e.components};return(0,a.jsxs)(a.Fragment,{children:[(0,a.jsx)(n.p,{children:"This functionality allows to execute financial transactions based on predefined parameters and templates. The specific transaction being posted here is based on a deposit template, which facilitates adding funds to a user's account."}),"\n",(0,a.jsx)(n.h2,{id:"process",children:"Process"}),"\n",(0,a.jsx)(n.h3,{id:"variables",children:"Variables"}),"\n",(0,a.jsxs)(n.ul,{children:["\n",(0,a.jsxs)(n.li,{children:[(0,a.jsx)(n.strong,{children:"Transaction ID"}),": Each transaction is uniquely identified by a ",(0,a.jsx)(n.code,{children:"transactionId"}),", ensuring that each transaction can be tracked individually and is distinct."]}),"\n",(0,a.jsxs)(n.li,{children:[(0,a.jsx)(n.strong,{children:"Template Code"}),": The ",(0,a.jsx)(n.code,{children:"txTemplateCode"})," specifies the template to use for the transaction, in this case, a deposit. This ensures that the transaction adheres to predefined rules and parameters for deposits."]}),"\n",(0,a.jsxs)(n.li,{children:[(0,a.jsx)(n.strong,{children:"Parameters"}),": The ",(0,a.jsx)(n.code,{children:"params"})," specify the particular details for the transaction, such as the account to which the deposit is made, the amount of the deposit, and the effective date of the transaction. These details are crucial for the accurate execution of the transaction according to the user\u2019s needs and timing requirements."]}),"\n"]}),"\n",(0,a.jsx)(n.pre,{children:(0,a.jsx)(n.code,{className:"language-json",children:'{\n  "input": {\n    "transactionId": "204d087b-5b6d-4544-9203-6674d54528d3",\n    "txTemplateCode": "DEPOSIT-ea1c7224-ca09-409f-b581-3551beead58c",\n    "params": {\n      "account": "3a7d421b-7f5a-43ca-ba6f-5f3e6ee67237",\n      "amount": "9.53",\n      "effective": "2022-09-21"\n    }\n  }\n}\n'})}),"\n",(0,a.jsx)(n.h3,{id:"graphql-request-body",children:"GraphQL Request Body"}),"\n",(0,a.jsxs)(n.p,{children:["The ",(0,a.jsx)(n.code,{children:"transactionPost"})," mutation is called with the inputs below. This mutation processes the transaction based on the provided template and parameters."]}),"\n",(0,a.jsx)(n.pre,{children:(0,a.jsx)(n.code,{className:"language-graphql",children:"mutation transactionPost($input: TransactionInput!) {\n  transactionPost(input: $input) {\n    transaction {\n      transactionId\n      correlationId\n    }\n  }\n}\n"})}),"\n",(0,a.jsx)(n.p,{children:"The system validates the input data against the specified template, calculates any necessary values or triggers other business logic as defined by the template, and logs the transaction in the appropriate accounts."}),"\n",(0,a.jsx)(n.h2,{id:"response",children:"Response"}),"\n",(0,a.jsx)(n.p,{children:"Upon successful processing of the mutation, the system returns the transaction ID and a correlation ID. The correlation ID can be used to track the transaction through other systems or logs for auditing or debugging purposes. This ensures traceability and accountability of the transaction."}),"\n",(0,a.jsx)(n.pre,{children:(0,a.jsx)(n.code,{className:"language-json",children:'{\n  "data": {\n    "transactionPost": {\n      "transaction": {\n        "transactionId": "204d087b-5b6d-4544-9203-6674d54528d3",\n        "correlationId": "204d087b-5b6d-4544-9203-6674d54528d3"\n      }\n    }\n  }\n}\n'})}),"\n",(0,a.jsx)(n.h2,{id:"significance",children:"Significance"}),"\n",(0,a.jsx)(n.p,{children:"Posting transactions in a controlled and templated manner reduces errors and ensures consistency in transaction handling. It allows financial institutions or businesses to handle financial transactions systematically, providing clarity and reliability in financial operations. This process is particularly important in environments where accuracy and consistency in financial transactions are critical for compliance and operational integrity."})]})}function h(e={}){const{wrapper:n}={...(0,s.R)(),...e.components};return n?(0,a.jsx)(n,{...e,children:(0,a.jsx)(l,{...e})}):l(e)}}}]);