"use strict";(self.webpackChunkwebsite=self.webpackChunkwebsite||[]).push([[799],{1034:(e,n,c)=>{c.r(n),c.d(n,{assets:()=>o,contentTitle:()=>r,default:()=>u,frontMatter:()=>s,metadata:()=>t,toc:()=>l});var a=c(4848),i=c(8453);const s={id:"check-account-balance",title:"Check account balance",slug:"/docs/demo/check-account-balance"},r=void 0,t={id:"demo/check-account-balance",title:"Check account balance",description:"The functionality is essential for users (e.g., account holders, financial managers) to view the balance of a specific account in a particular journal and currency. This allows for real-time financial monitoring and decision-making based on up-to-date account information.",source:"@site/docs/demo/check-account-balance.mdx",sourceDirName:"demo",slug:"/docs/demo/check-account-balance",permalink:"/docs/docs/demo/check-account-balance",draft:!1,unlisted:!1,editUrl:"https://github.com/GaloyMoney/cala/edit/main//website/docs/demo/check-account-balance.mdx",tags:[],version:"current",frontMatter:{id:"check-account-balance",title:"Check account balance",slug:"/docs/demo/check-account-balance"},sidebar:"demoSidebar",previous:{title:"Post a transaction",permalink:"/docs/docs/demo/post-transaction"}},o={},l=[{value:"Process",id:"process",level:2},{value:"Variables",id:"variables",level:3},{value:"GraphQL body",id:"graphql-body",level:3},{value:"Response",id:"response",level:2},{value:"Significance",id:"significance",level:2}];function d(e){const n={code:"code",h2:"h2",h3:"h3",li:"li",p:"p",pre:"pre",strong:"strong",ul:"ul",...(0,i.R)(),...e.components};return(0,a.jsxs)(a.Fragment,{children:[(0,a.jsx)(n.p,{children:"The functionality is essential for users (e.g., account holders, financial managers) to view the balance of a specific account in a particular journal and currency. This allows for real-time financial monitoring and decision-making based on up-to-date account information."}),"\n",(0,a.jsx)(n.h2,{id:"process",children:"Process"}),"\n",(0,a.jsx)(n.h3,{id:"variables",children:"Variables"}),"\n",(0,a.jsxs)(n.ul,{children:["\n",(0,a.jsxs)(n.li,{children:[(0,a.jsx)(n.strong,{children:"Account ID"}),": The ",(0,a.jsx)(n.code,{children:"accountId"})," uniquely identifies the account whose balance is being queried. This ensures that the query is precise and retrieves information for the correct account."]}),"\n",(0,a.jsxs)(n.li,{children:[(0,a.jsx)(n.strong,{children:"Journal ID"}),": The ",(0,a.jsx)(n.code,{children:"journalId"})," specifies which journal to check for the account's balance. This is important because an account may have different balances in different journals due to various types of transactions."]}),"\n",(0,a.jsxs)(n.li,{children:[(0,a.jsx)(n.strong,{children:"Currency"}),": The ",(0,a.jsx)(n.code,{children:"currency"})," parameter ensures that the balance is provided in the desired currency, in this case, USD. This is crucial for accuracy and relevance, especially in multi-currency environments."]}),"\n"]}),"\n",(0,a.jsx)(n.pre,{children:(0,a.jsx)(n.code,{children:'{\n  "accountId": "3a7d421b-7f5a-43ca-ba6f-5f3e6ee67237",\n  "journalId": "bcc24f47-990c-457d-88cb-76332450ac77",\n  "currency": "USD"\n}\n'})}),"\n",(0,a.jsx)(n.h3,{id:"graphql-body",children:"GraphQL body"}),"\n",(0,a.jsxs)(n.p,{children:["The ",(0,a.jsx)(n.code,{children:"accountWithBalance"})," query is executed with the provided inputs. The query fetches the account's name and its settled balance in the specified journal and currency."]}),"\n",(0,a.jsx)(n.pre,{children:(0,a.jsx)(n.code,{className:"language-graphql",children:"query accountWithBalance(\n  $accountId: UUID!\n  $journalId: UUID!\n  $currency: CurrencyCode!\n) {\n  account(id: $accountId) {\n    name\n    balance(journalId: $journalId, currency: $currency) {\n      settled {\n        normalBalance {\n          units\n        }\n      }\n    }\n  }\n}\n"})}),"\n",(0,a.jsx)(n.p,{children:"The system retrieves the settled balance from the specified journal for the given account."}),"\n",(0,a.jsx)(n.h2,{id:"response",children:"Response"}),"\n",(0,a.jsx)(n.p,{children:"The response includes the account's name and its settled balance in the specified currency and journal. This information is returned in a structured JSON format, which includes:"}),"\n",(0,a.jsxs)(n.ul,{children:["\n",(0,a.jsxs)(n.li,{children:[(0,a.jsx)(n.strong,{children:"Account Name"}),': "Alice - Checking", confirming that the balance belongs to the correct account.']}),"\n",(0,a.jsxs)(n.li,{children:[(0,a.jsx)(n.strong,{children:"Settled Balance"}),": The ",(0,a.jsx)(n.code,{children:"normalBalance"})," ",(0,a.jsx)(n.code,{children:"units"}),' show the account\'s balance as "9.53" USD, indicating the available settled funds in the account.']}),"\n"]}),"\n",(0,a.jsx)(n.pre,{children:(0,a.jsx)(n.code,{className:"language-json",children:'{\n  "data": {\n    "account": {\n      "name": "Alice - Checking",\n      "balance": {\n        "settled": {\n          "normalBalance": {\n            "units": "9.53"\n          }\n        }\n      }\n    }\n  }\n}\n'})}),"\n",(0,a.jsx)(n.h2,{id:"significance",children:"Significance"}),"\n",(0,a.jsx)(n.p,{children:"Checking account balances is a fundamental operation in financial management. It allows users to:"}),"\n",(0,a.jsxs)(n.ul,{children:["\n",(0,a.jsxs)(n.li,{children:[(0,a.jsx)(n.strong,{children:"Monitor Financial Status"}),": Users can keep track of their available funds, ensuring they are aware of their financial position."]}),"\n",(0,a.jsxs)(n.li,{children:[(0,a.jsx)(n.strong,{children:"Make Informed Decisions"}),": Accurate and up-to-date balance information is essential for making financial decisions, such as initiating transactions, budgeting, or investing."]}),"\n",(0,a.jsxs)(n.li,{children:[(0,a.jsx)(n.strong,{children:"Ensure Compliance and Accuracy"}),": Regularly checking balances helps in identifying any discrepancies or issues early, maintaining the integrity of financial records."]}),"\n"]})]})}function u(e={}){const{wrapper:n}={...(0,i.R)(),...e.components};return n?(0,a.jsx)(n,{...e,children:(0,a.jsx)(d,{...e})}):d(e)}}}]);